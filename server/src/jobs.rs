//! Durable background jobs (also the transactional outbox for notifications and integrations).
//!
//! * [`enqueue`] inside the caller's write transaction — the job exists iff the business change committed.
//! * The worker ([`run_worker`]) claims one ready job at a time with a 60 s lease in a short write
//!   transaction, runs the handler **outside** any transaction, then marks it `done`, reschedules it with
//!   exponential backoff (`min(5 s · 2^(attempts-1), 1 h)` ± 20 % jitter) or marks it `dead` after
//!   `max_attempts`.
//! * Handlers are dispatched by kind prefix (see [`dispatch`]). A handler must be idempotent: it may run
//!   again after a crash or an expired lease.
//! * The scheduler ([`run_scheduler`]) enqueues `deadline.sweep` every minute and, in demo mode,
//!   `demo.reset` every `DEMO_RESET_HOURS`.

use std::time::Duration;

use chrono::{DateTime, Utc};
use rand::Rng;
use serde_json::Value;
use sqlx::SqliteConnection;

use crate::db::write_tx;
use crate::error::{AppError, AppResult};
use crate::settings;
use crate::state::AppState;
use crate::time;

/// Lease length for a claimed job.
pub const LEASE_SECS: i64 = 60;

/// Queues a job. With `idempotency_key = Some(k)` the call is a no-op if a job with that key exists.
pub async fn enqueue(
    conn: &mut SqliteConnection,
    kind: &str,
    payload: Value,
    idempotency_key: Option<String>,
    run_after: DateTime<Utc>,
) -> AppResult<()> {
    let now = time::now_str();
    sqlx::query(
        "INSERT INTO jobs (kind, payload_json, idempotency_key, status, run_after, created_at, updated_at) \
         VALUES (?, ?, ?, 'pending', ?, ?, ?) ON CONFLICT(idempotency_key) DO NOTHING",
    )
    .bind(kind)
    .bind(payload.to_string())
    .bind(idempotency_key)
    .bind(time::fmt(run_after))
    .bind(&now)
    .bind(&now)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Routes a job to its owning module by kind prefix.
pub async fn dispatch(state: &AppState, kind: &str, payload: &Value) -> AppResult<()> {
    if kind.starts_with("notify.") {
        crate::notify::handle_job(state, kind, payload).await
    } else if kind.starts_with("integration.") || kind.starts_with("records.") {
        crate::records::handle_job(state, kind, payload).await
    } else if kind.starts_with("deadline.") {
        crate::deadlines::handle_job(state, kind, payload).await
    } else if kind.starts_with("finance.") {
        crate::finance::handle_job(state, kind, payload).await
    } else if kind.starts_with("ops.") {
        crate::operations::handle_job(state, kind, payload).await
    } else if kind == "demo.reset" {
        crate::seed::reset_demo(state).await
    } else {
        Err(AppError::internal(format!("no handler for job kind {kind}")))
    }
}

/// Backoff before retry number `attempts` (1 = after the first failure): `min(5 s · 2^(attempts-1), 1 h)`,
/// then ±20 % jitter.
pub fn backoff(attempts: i64) -> chrono::Duration {
    let exp = (attempts - 1).clamp(0, 20) as u32;
    let base = (5i64 * 2i64.pow(exp)).min(3600) as f64;
    let jitter = rand::thread_rng().gen_range(0.8..=1.2);
    chrono::Duration::milliseconds((base * jitter * 1000.0) as i64)
}

#[derive(Debug, sqlx::FromRow)]
struct ClaimedJob {
    id: i64,
    kind: String,
    payload_json: String,
    attempts: i64,
    max_attempts: i64,
}

/// Claims and runs at most one ready job. Returns `Ok(true)` if a job was processed.
pub async fn run_once(state: &AppState) -> AppResult<bool> {
    let now = state.now();
    let now_s = time::fmt(now);
    let lease = time::fmt(now + chrono::Duration::seconds(LEASE_SECS));

    let mut tx = write_tx(&state.db).await?;
    let job: Option<ClaimedJob> = sqlx::query_as(
        "SELECT id, kind, payload_json, attempts, max_attempts FROM jobs \
         WHERE (status = 'pending' AND run_after <= ?) OR (status = 'running' AND lease_until < ?) \
         ORDER BY run_after, id LIMIT 1",
    )
    .bind(&now_s)
    .bind(&now_s)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(job) = job else {
        return Ok(false);
    };
    sqlx::query(
        "UPDATE jobs SET status = 'running', lease_until = ?, attempts = attempts + 1, updated_at = ? WHERE id = ?",
    )
    .bind(&lease)
    .bind(&now_s)
    .bind(job.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let attempts = job.attempts + 1;
    let payload: Value = serde_json::from_str(&job.payload_json).unwrap_or(Value::Null);
    let result = dispatch(state, &job.kind, &payload).await;

    let done_at = time::fmt(state.now());
    let mut tx = write_tx(&state.db).await?;
    match result {
        Ok(()) => {
            sqlx::query(
                "UPDATE jobs SET status = 'done', lease_until = NULL, last_error = NULL, updated_at = ? WHERE id = ?",
            )
            .bind(&done_at)
            .bind(job.id)
            .execute(&mut *tx)
            .await?;
        }
        Err(e) => {
            let msg = e.message.clone();
            if attempts >= job.max_attempts {
                tracing::warn!(job_id = job.id, kind = %job.kind, error = %msg, "job is dead after {attempts} attempts");
                sqlx::query(
                    "UPDATE jobs SET status = 'dead', lease_until = NULL, last_error = ?, updated_at = ? WHERE id = ?",
                )
                .bind(&msg)
                .bind(&done_at)
                .bind(job.id)
                .execute(&mut *tx)
                .await?;
            } else {
                let retry_at = time::fmt(state.now() + backoff(attempts));
                tracing::info!(job_id = job.id, kind = %job.kind, error = %msg, retry_at = %retry_at, "job failed; will retry");
                sqlx::query(
                    "UPDATE jobs SET status = 'pending', lease_until = NULL, last_error = ?, run_after = ?, updated_at = ? WHERE id = ?",
                )
                .bind(&msg)
                .bind(&retry_at)
                .bind(&done_at)
                .bind(job.id)
                .execute(&mut *tx)
                .await?;
            }
        }
    }
    tx.commit().await?;
    Ok(true)
}

/// Worker loop: processes jobs back-to-back while any are ready, otherwise polls every second.
pub async fn run_worker(state: AppState) {
    loop {
        match run_once(&state).await {
            Ok(true) => continue,
            Ok(false) => tokio::time::sleep(Duration::from_secs(1)).await,
            Err(e) => {
                tracing::error!(error = %e.message, "job worker error");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// One scheduler tick: `deadline.sweep` for the current minute and, when due, `demo.reset`.
pub async fn schedule_tick(state: &AppState) -> AppResult<()> {
    let now = state.now();
    let mut tx = write_tx(&state.db).await?;
    let minute = now.format("%Y-%m-%dT%H:%M").to_string();
    enqueue(&mut tx, "deadline.sweep", serde_json::json!({}), Some(format!("deadline.sweep:{minute}")), now).await?;
    if state.cfg.demo_mode
        && let Some(next) = next_demo_reset(&mut tx, state.cfg.demo_reset_hours).await?
        && now >= next
    {
        enqueue(&mut tx, "demo.reset", serde_json::json!({}), Some(format!("demo.reset:{}", time::fmt(next))), now)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Next scheduled demo reset: `demo.last_reset_at + hours`, or `None` if the demo was never seeded.
pub async fn next_demo_reset(conn: &mut SqliteConnection, hours: u32) -> AppResult<Option<DateTime<Utc>>> {
    if hours == 0 {
        return Ok(None);
    }
    let last: Option<String> = settings::get(conn, settings::keys::DEMO_LAST_RESET_AT).await?;
    Ok(match last {
        Some(s) => Some(time::parse(&s)? + chrono::Duration::hours(i64::from(hours))),
        None => None,
    })
}

/// Scheduler loop: one tick per minute.
pub async fn run_scheduler(state: AppState) {
    loop {
        if let Err(e) = schedule_tick(&state).await {
            tracing::error!(error = %e.message, "scheduler error");
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Clock;
    use chrono::TimeZone;

    #[test]
    fn backoff_grows_and_caps() {
        for _ in 0..20 {
            let b1 = backoff(1).num_milliseconds();
            assert!((4000..=6000).contains(&b1), "{b1}");
            let b3 = backoff(3).num_milliseconds();
            assert!((16000..=24000).contains(&b3), "{b3}");
            let b20 = backoff(20).num_milliseconds();
            assert!((2_880_000..=4_320_000).contains(&b20), "{b20}");
        }
    }

    #[tokio::test]
    async fn retries_with_backoff_then_dead() {
        let t0 = Utc.with_ymd_and_hms(2026, 10, 7, 0, 0, 0).unwrap();
        let (state, clock, _dir) = crate::state::test_support::test_state_fixed(t0).await;
        let mut tx = write_tx(&state.db).await.unwrap();
        enqueue(&mut tx, "test.always_fails", serde_json::json!({"x": 1}), Some("k1".into()), t0).await.unwrap();
        // Same idempotency key → no second job.
        enqueue(&mut tx, "test.always_fails", serde_json::json!({"x": 2}), Some("k1".into()), t0).await.unwrap();
        sqlx::query("UPDATE jobs SET max_attempts = 3").execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs").fetch_one(&state.db).await.unwrap();
        assert_eq!(count, 1);

        // Attempt 1 fails → pending, retry ~5 s later.
        assert!(run_once(&state).await.unwrap());
        let (status, attempts, run_after, err): (String, i64, String, Option<String>) =
            sqlx::query_as("SELECT status, attempts, run_after, last_error FROM jobs")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!((status.as_str(), attempts), ("pending", 1));
        assert!(err.unwrap().contains("no handler"));
        let delay = (time::parse(&run_after).unwrap() - t0).num_milliseconds();
        assert!((4000..=6000).contains(&delay), "first backoff {delay}");
        // Not ready yet.
        assert!(!run_once(&state).await.unwrap());

        // Attempt 2 → retry ~10 s later.
        clock.advance(chrono::Duration::seconds(7));
        assert!(run_once(&state).await.unwrap());
        let (status, attempts, run_after): (String, i64, String) =
            sqlx::query_as("SELECT status, attempts, run_after FROM jobs").fetch_one(&state.db).await.unwrap();
        assert_eq!((status.as_str(), attempts), ("pending", 2));
        let delay = (time::parse(&run_after).unwrap() - clock.now()).num_milliseconds();
        assert!((8000..=12000).contains(&delay), "second backoff {delay}");

        // Attempt 3 = max_attempts → dead.
        clock.advance(chrono::Duration::seconds(13));
        assert!(run_once(&state).await.unwrap());
        let (status, attempts): (String, i64) =
            sqlx::query_as("SELECT status, attempts FROM jobs").fetch_one(&state.db).await.unwrap();
        assert_eq!((status.as_str(), attempts), ("dead", 3));
        clock.advance(chrono::Duration::hours(2));
        assert!(!run_once(&state).await.unwrap());
    }

    #[tokio::test]
    async fn expired_lease_is_reclaimed_and_scheduler_dedupes() {
        let t0 = Utc.with_ymd_and_hms(2026, 10, 7, 0, 0, 0).unwrap();
        let (state, clock, _dir) = crate::state::test_support::test_state_fixed(t0).await;
        sqlx::query(
            "INSERT INTO jobs (kind, payload_json, status, attempts, run_after, lease_until, created_at, updated_at) \
             VALUES ('deadline.sweep', '{}', 'running', 1, ?, ?, ?, ?)",
        )
        .bind(time::fmt(t0))
        .bind(time::fmt(t0 + chrono::Duration::seconds(30)))
        .bind(time::fmt(t0))
        .bind(time::fmt(t0))
        .execute(&state.db)
        .await
        .unwrap();
        assert!(!run_once(&state).await.unwrap(), "lease still valid");
        clock.advance(chrono::Duration::seconds(61));
        assert!(run_once(&state).await.unwrap(), "expired lease reclaimed");
        let status: String = sqlx::query_scalar("SELECT status FROM jobs").fetch_one(&state.db).await.unwrap();
        assert_eq!(status, "done");

        schedule_tick(&state).await.unwrap();
        schedule_tick(&state).await.unwrap();
        let sweeps: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE idempotency_key LIKE 'deadline.sweep:%'")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(sweeps, 1);
    }
}
