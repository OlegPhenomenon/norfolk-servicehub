//! Transactional retention and external-system outbox API.
use crate::{
    cases::core::CaseRow,
    error::{AppError, AppResult},
    time,
};
use chrono::{Datelike, NaiveDate};
use serde_json::json;
use sqlx::SqliteConnection;

pub fn record_class(module: &str) -> &str {
    match module {
        "building" | "planning_certificate" => "building",
        "venue_booking" | "equipment_hire" => "hire",
        "complaint" => "complaint",
        "road_issue" => "works",
        _ => "default",
    }
}
fn anniversary(date: NaiveDate, years: i32) -> AppResult<NaiveDate> {
    let year = date.year().checked_add(years).ok_or_else(|| AppError::internal("retention year overflow"))?;
    NaiveDate::from_ymd_opt(year, date.month(), date.day())
        .or_else(|| NaiveDate::from_ymd_opt(year, date.month(), 28))
        .ok_or_else(|| AppError::internal("retention date outside supported range"))
}
pub async fn on_case_closed(tx: &mut SqliteConnection, case: &CaseRow) -> AppResult<()> {
    let years: i32 = sqlx::query_scalar("SELECT retain_years FROM retention_rules WHERE record_class IN (?, 'default') AND trigger_event = 'case_closed' ORDER BY record_class = ? DESC LIMIT 1")
        .bind(record_class(&case.module)).bind(record_class(&case.module)).fetch_optional(&mut *tx).await?.unwrap_or(7);
    let closed = case.closed_at.as_deref().ok_or_else(|| AppError::conflict("The case is still open."))?;
    let until = time::fmt_date(anniversary(time::local_date(time::parse(closed)?), years)?);
    let changed = sqlx::query(
        "UPDATE cases SET retention_until = ? WHERE id = ? AND (retention_until IS NULL OR retention_until < ?)",
    )
    .bind(&until)
    .bind(case.id)
    .bind(&until)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed > 0 {
        crate::cases::core::append_event(
            tx,
            case.id,
            None,
            "records.retention_set",
            crate::cases::core::Visibility::Staff,
            "Retention date set or extended for the closed request.",
            json!({"retention_until":until,"previous_retention_until":case.retention_until}),
        )
        .await?;
        crate::audit::record(
            tx,
            None,
            "records.retention_set",
            "case",
            Some(case.id),
            json!({"retention_until":until,"previous_retention_until":case.retention_until}),
        )
        .await?;
    }
    let closure_id: i64 = sqlx::query_scalar("INSERT INTO case_closure_events(case_id,generation,closed_at) VALUES(?,?,?) ON CONFLICT(case_id,generation) DO UPDATE SET case_id=excluded.case_id RETURNING id")
        .bind(case.id).bind(case.reopened_count).bind(closed).fetch_one(&mut *tx).await?;
    let retained: String =
        sqlx::query_scalar("SELECT retention_until FROM cases WHERE id=?").bind(case.id).fetch_one(&mut *tx).await?;
    super::integrations::enqueue(
        tx,
        "content_manager",
        Some(case.id),
        "record.case_closed",
        closure_id,
        json!({"case":case,"retention_until":retained,"closure_event_id":closure_id,"demo":true}),
    )
    .await
}
pub async fn on_decision_issued(tx: &mut SqliteConnection, case_id: i64, decision_id: i64) -> AppResult<()> {
    let decision = super::common::rows(
        tx,
        "SELECT * FROM decisions WHERE id = ? AND case_id = ? AND status = 'issued'",
        &[crate::db::SqlValue::Int(decision_id), crate::db::SqlValue::Int(case_id)],
    )
    .await?;
    let decision = decision.first().ok_or_else(AppError::not_found)?;
    super::integrations::enqueue(
        tx,
        "content_manager",
        Some(case_id),
        "document.decision",
        decision_id,
        json!({"decision":decision,"demo":true}),
    )
    .await
}
pub async fn on_payment_confirmed(tx: &mut SqliteConnection, payment_id: i64) -> AppResult<()> {
    let payment = super::common::rows(
        tx,
        "SELECT * FROM payments WHERE id = ? AND status = 'confirmed'",
        &[crate::db::SqlValue::Int(payment_id)],
    )
    .await?;
    let payment = payment.first().ok_or_else(AppError::not_found)?;
    super::integrations::enqueue(
        tx,
        "civica_altitude",
        payment["case_id"].as_i64(),
        "payment.receipt",
        payment_id,
        json!({"payment":payment,"demo":true}),
    )
    .await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leap_day_retention() {
        assert_eq!(anniversary(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap(), 7).unwrap().to_string(), "2031-02-28");
        assert_eq!(record_class("planning_certificate"), "building");
        assert_eq!(record_class("generic"), "default");
    }
}
