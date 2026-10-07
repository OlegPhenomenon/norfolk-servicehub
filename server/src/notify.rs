//! Notifications: in-app bell rows plus outbound email/SMS delivered by a job through the DemoMail mock
//! gateway (`POST {INTERNAL_BASE_URL}/mock/mail/send`).

use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqliteConnection;

use crate::error::{AppError, AppResult};
use crate::jobs;
use crate::state::AppState;
use crate::time;

/// What to tell whom. Any combination of `user_id` (in-app), `email` and `phone` (SMS) may be set.
#[derive(Debug, Clone, Default)]
pub struct Notice {
    /// Account to show the in-app notification to.
    pub user_id: Option<i64>,
    /// Outbound email address.
    pub email: Option<String>,
    /// Outbound SMS number.
    pub phone: Option<String>,
    pub case_id: Option<i64>,
    pub subject: String,
    pub body: String,
    /// SPA path, e.g. `/my/cases/12`.
    pub link: Option<String>,
}

/// Inserts the in-app row (if `user_id`) and queues `email` / `sms` rows with a `notify.deliver` job each.
pub async fn send(conn: &mut SqliteConnection, notice: Notice) -> AppResult<()> {
    let now = time::now_str();
    if let Some(uid) = notice.user_id {
        insert(conn, Some(uid), "in_app", None, &notice, "sent", &now).await?;
    }
    for (channel, addr) in [("email", &notice.email), ("sms", &notice.phone)] {
        let Some(addr) = addr.as_deref().map(str::trim).filter(|a| !a.is_empty()) else { continue };
        let id = insert(conn, notice.user_id, channel, Some(addr), &notice, "queued", &now).await?;
        jobs::enqueue(
            conn,
            "notify.deliver",
            json!({ "notification_id": id }),
            Some(format!("notify.deliver:{id}")),
            chrono::Utc::now(),
        )
        .await?;
    }
    Ok(())
}

async fn insert(
    conn: &mut SqliteConnection,
    user_id: Option<i64>,
    channel: &str,
    to: Option<&str>,
    n: &Notice,
    status: &str,
    now: &str,
) -> AppResult<i64> {
    let sent_at = (status == "sent").then_some(now);
    Ok(sqlx::query_scalar(
        "INSERT INTO notifications (user_id, channel, to_address, case_id, subject, body, link, status, created_at, sent_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(user_id)
    .bind(channel)
    .bind(to)
    .bind(n.case_id)
    .bind(&n.subject)
    .bind(&n.body)
    .bind(&n.link)
    .bind(status)
    .bind(now)
    .bind(sent_at)
    .fetch_one(&mut *conn)
    .await?)
}

#[derive(Debug, sqlx::FromRow)]
struct Outbound {
    id: i64,
    channel: String,
    to_address: Option<String>,
    subject: String,
    body: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct SendResponse {
    id: String,
}

/// Job handler for `notify.*` kinds.
pub async fn handle_job(state: &AppState, kind: &str, payload: &Value) -> AppResult<()> {
    match kind {
        "notify.deliver" => {
            let id = payload["notification_id"]
                .as_i64()
                .ok_or_else(|| AppError::internal("notify.deliver without notification_id"))?;
            deliver(state, id).await
        }
        other => Err(AppError::internal(format!("unknown notify job {other}"))),
    }
}

/// Sends one queued email/SMS through DemoMail. Permanent gateway rejections (422) mark the
/// notification `failed`; transient errors return `Err` so the job retries.
async fn deliver(state: &AppState, id: i64) -> AppResult<()> {
    let n: Option<Outbound> =
        sqlx::query_as("SELECT id, channel, to_address, subject, body, status FROM notifications WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let Some(n) = n else { return Ok(()) };
    if n.status != "queued" {
        return Ok(());
    }
    let url = format!("{}/mock/mail/send", state.cfg.internal_base_url);
    let res = state
        .http
        .post(&url)
        .header("X-Mock-Key", &state.cfg.mock_api_key)
        .json(&json!({ "channel": n.channel, "to": n.to_address, "subject": n.subject, "body": n.body }))
        .send()
        .await;
    let now = time::fmt(state.now());
    match res {
        Ok(r) if r.status().is_success() => {
            let body: SendResponse =
                r.json().await.map_err(|e| AppError::internal(format!("DemoMail response: {e}")))?;
            sqlx::query(
                "UPDATE notifications SET status = 'sent', external_id = ?, sent_at = ?, attempts = attempts + 1, last_error = NULL \
                 WHERE id = ? AND status = 'queued'",
            )
            .bind(body.id)
            .bind(&now)
            .bind(n.id)
            .execute(&state.db)
            .await?;
            Ok(())
        }
        Ok(r) if r.status() == reqwest::StatusCode::UNPROCESSABLE_ENTITY => {
            let body: Value = r.json().await.unwrap_or(Value::Null);
            let msg = body["error"]["message"].as_str().unwrap_or("Rejected by the gateway").to_string();
            sqlx::query(
                "UPDATE notifications SET status = 'failed', last_error = ?, attempts = attempts + 1 WHERE id = ? AND status = 'queued'",
            )
            .bind(&msg)
            .bind(n.id)
            .execute(&state.db)
            .await?;
            Ok(())
        }
        Ok(r) => {
            let msg = format!("DemoMail returned HTTP {}", r.status());
            record_attempt(state, n.id, &msg).await?;
            Err(AppError::internal(msg))
        }
        Err(e) => {
            let msg = format!("DemoMail unreachable: {e}");
            record_attempt(state, n.id, &msg).await?;
            Err(AppError::internal(msg))
        }
    }
}

async fn record_attempt(state: &AppState, id: i64, msg: &str) -> AppResult<()> {
    sqlx::query("UPDATE notifications SET attempts = attempts + 1, last_error = ? WHERE id = ?")
        .bind(msg)
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::write_tx;

    /// Full outbox cycle against the real DemoMail route served on a random port.
    #[tokio::test]
    async fn delivers_through_demomail_and_records_bounces() {
        let (state, _dir) = crate::state::test_support::test_state().await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut cfg = (*state.cfg).clone();
        cfg.internal_base_url = format!("http://{addr}");
        let state = AppState { cfg: std::sync::Arc::new(cfg), ..state };
        let app = crate::app::build_router(state.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let mut tx = write_tx(&state.db).await.unwrap();
        for email in ["resident@example.org", "nobody@bounce.example"] {
            send(
                &mut tx,
                Notice { email: Some(email.into()), subject: "Hello".into(), body: "Body".into(), ..Notice::default() },
            )
            .await
            .unwrap();
        }
        tx.commit().await.unwrap();

        while jobs::run_once(&state).await.unwrap() {}

        let rows: Vec<(String, String, Option<String>, Option<String>)> =
            sqlx::query_as("SELECT to_address, status, external_id, last_error FROM notifications ORDER BY id")
                .fetch_all(&state.db)
                .await
                .unwrap();
        assert_eq!(rows[0].1, "sent");
        assert!(rows[0].2.as_deref().unwrap().starts_with("dm_"));
        assert_eq!(rows[1].1, "failed");
        assert!(rows[1].3.as_deref().unwrap().contains("permanent"), "{rows:?}");
        let done: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE status = 'done'").fetch_one(&state.db).await.unwrap();
        assert_eq!(done, 2);
    }
}
