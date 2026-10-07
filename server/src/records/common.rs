//! Shared records queries and command history.
use crate::{
    audit,
    auth::Actor,
    authz::Role,
    cases::core,
    db::{self, SqlValue},
    error::{AppError, AppResult},
};
use serde_json::{Value, json};
use sqlx::{Column, Row, SqliteConnection, TypeInfo, ValueRef};

pub fn require_role(actor: &Actor, role: Role) -> AppResult<()> {
    if !actor.is_staff() {
        return Err(AppError::forbidden());
    }
    actor.require_any_role(&[role])
}
pub fn text(value: &str, field: &str, max: usize) -> AppResult<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        return Err(AppError::field(field, format!("Enter between 1 and {max} characters.")));
    }
    Ok(value.into())
}
pub fn email(value: &str) -> AppResult<String> {
    let value = text(value, "email", 254)?.to_lowercase();
    if !value.contains('@') || value.contains(char::is_whitespace) || value.starts_with('@') || value.ends_with('@') {
        return Err(AppError::field("email", "Enter a valid email address."));
    }
    Ok(value)
}
pub async fn rows(conn: &mut SqliteConnection, sql: &str, binds: &[SqlValue]) -> AppResult<Vec<Value>> {
    let rows = db::bind_all(sqlx::query(sql), binds).fetch_all(conn).await?;
    rows.into_iter()
        .map(|row| {
            let mut object = serde_json::Map::new();
            for col in row.columns() {
                let i = col.ordinal();
                let raw = row.try_get_raw(i)?;
                let value = if raw.is_null() {
                    Value::Null
                } else {
                    match raw.type_info().name() {
                        "INTEGER" | "BOOLEAN" => json!(row.try_get::<i64, _>(i)?),
                        "REAL" => json!(row.try_get::<f64, _>(i)?),
                        _ => json!(row.try_get::<String, _>(i)?),
                    }
                };
                object.insert(col.name().to_string(), value);
            }
            Ok(Value::Object(object))
        })
        .collect()
}
pub fn check_revision(case: &core::CaseRow, expected: Option<i64>) -> AppResult<()> {
    if expected.is_some_and(|revision| revision != case.revision) {
        return Err(AppError::stale_revision());
    }
    Ok(())
}
pub async fn changed(
    conn: &mut SqliteConnection,
    actor: &Actor,
    case_id: i64,
    action: &str,
    summary: &str,
    data: Value,
) -> AppResult<()> {
    core::bump_revision(conn, case_id, None).await?;
    core::append_event(conn, case_id, actor.db_id(), action, core::Visibility::Staff, summary, data.clone()).await?;
    audit::record(conn, actor.db_id(), action, "case", Some(case_id), data).await
}
pub async fn admin_audit(
    conn: &mut SqliteConnection,
    actor: &Actor,
    action: &str,
    entity: &str,
    id: Option<i64>,
    data: Value,
) -> AppResult<()> {
    audit::record(conn, actor.db_id(), action, entity, id, data).await
}
