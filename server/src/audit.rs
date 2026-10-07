//! Audit trail. Every state-changing command writes one `audit_log` row (and, when case-related,
//! a `case_events` row via `cases::core::append_event`).

use serde_json::Value;
use sqlx::SqliteConnection;

use crate::error::AppResult;
use crate::time;

/// Records an audit entry inside the caller's transaction.
///
/// * `actor` — `users.id`, or `None` for system actions (`Actor::db_id()`).
/// * `action` — dotted verb, e.g. `"case.submit"`, `"refund.complete"`.
/// * `entity_type` / `entity_id` — what was changed, e.g. `("case", Some(12))`.
pub async fn record(
    conn: &mut SqliteConnection,
    actor: Option<i64>,
    action: &str,
    entity_type: &str,
    entity_id: Option<i64>,
    details: Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO audit_log (at, actor_user_id, action, entity_type, entity_id, details_json) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(time::now_str())
    .bind(actor)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(details.to_string())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Same as [`record`] including the client IP (login/logout and other security events).
pub async fn record_with_ip(
    conn: &mut SqliteConnection,
    actor: Option<i64>,
    action: &str,
    entity_type: &str,
    entity_id: Option<i64>,
    details: Value,
    ip: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO audit_log (at, actor_user_id, action, entity_type, entity_id, details_json, ip) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(time::now_str())
    .bind(actor)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(details.to_string())
    .bind(ip)
    .execute(&mut *conn)
    .await?;
    Ok(())
}
