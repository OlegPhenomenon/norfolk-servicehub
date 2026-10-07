//! Key/value settings (`settings` table, JSON values). Records' admin screens write settings through
//! these helpers.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::SqliteConnection;

use crate::error::AppResult;
use crate::time;

/// Well-known keys.
pub mod keys {
    /// Where staff-facing copies of customer emails go.
    pub const CUSTOMER_CARE_EMAIL: &str = "notify.customer_care_email";
    /// Demo reset interval in hours (informational copy of `DEMO_RESET_HOURS`).
    pub const DEMO_RESET_HOURS: &str = "demo.reset_hours";
    /// RFC 3339 instant of the last demo wipe + seed.
    pub const DEMO_LAST_RESET_AT: &str = "demo.last_reset_at";
}

/// Raw JSON value of a setting.
pub async fn get_json(conn: &mut SqliteConnection, key: &str) -> AppResult<Option<Value>> {
    let raw: Option<String> = sqlx::query_scalar("SELECT value_json FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(match raw {
        Some(s) => Some(serde_json::from_str(&s)?),
        None => None,
    })
}

/// Typed value of a setting (`None` if missing).
pub async fn get<T: DeserializeOwned>(conn: &mut SqliteConnection, key: &str) -> AppResult<Option<T>> {
    Ok(match get_json(conn, key).await? {
        Some(v) => Some(serde_json::from_value(v)?),
        None => None,
    })
}

/// Inserts or replaces a setting.
pub async fn set<T: Serialize>(
    conn: &mut SqliteConnection,
    key: &str,
    value: &T,
    updated_by: Option<i64>,
) -> AppResult<()> {
    let json = serde_json::to_string(value)?;
    sqlx::query(
        "INSERT INTO settings (key, value_json, updated_by, updated_at) VALUES (?, ?, ?, ?) \
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_by = excluded.updated_by, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(json)
    .bind(updated_by)
    .bind(time::now_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}
