//! Durable brute-force throttling over `login_attempts`: ≥ 10 failures for a key within 15 minutes →
//! `rate_limited`. Keys: `email:<lower email>`, `ip:<addr>`, `totp:<user_id>`.

use chrono::{DateTime, Duration, Utc};
use sqlx::SqliteConnection;

use crate::error::{AppError, AppResult};
use crate::time;

pub const MAX_FAILURES: i64 = 10;
pub const WINDOW_MINUTES: i64 = 15;

/// `rate_limited` if any key has too many recent failures.
pub async fn check(conn: &mut SqliteConnection, keys: &[String], now: DateTime<Utc>) -> AppResult<()> {
    let since = time::fmt(now - Duration::minutes(WINDOW_MINUTES));
    for key in keys {
        let failures: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM login_attempts WHERE key = ? AND at >= ? AND success = 0")
                .bind(key)
                .bind(&since)
                .fetch_one(&mut *conn)
                .await?;
        if failures >= MAX_FAILURES {
            return Err(AppError::rate_limited());
        }
    }
    Ok(())
}

/// Records one attempt for each key.
pub async fn record(conn: &mut SqliteConnection, keys: &[String], success: bool, now: DateTime<Utc>) -> AppResult<()> {
    let at = time::fmt(now);
    for key in keys {
        sqlx::query("INSERT INTO login_attempts (key, at, success) VALUES (?, ?, ?)")
            .bind(key)
            .bind(&at)
            .bind(i64::from(success))
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}
