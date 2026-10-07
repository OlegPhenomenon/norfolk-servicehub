// OWNER: records
#![allow(dead_code, unused_variables)]
//! Cross-module records API.

use sqlx::SqliteConnection;

use crate::cases::core::CaseRow;
use crate::error::{AppError, AppResult};

/// Sets `retention_until` and queues outbound record delivery.
pub async fn on_case_closed(tx: &mut SqliteConnection, case: &CaseRow) -> AppResult<()> {
    Err(AppError::internal("not implemented: records::api::on_case_closed"))
}

/// Queues delivery of an issued decision to the records system.
pub async fn on_decision_issued(tx: &mut SqliteConnection, case_id: i64, decision_id: i64) -> AppResult<()> {
    Err(AppError::internal("not implemented: records::api::on_decision_issued"))
}

/// Queues the receipt to the finance system.
pub async fn on_payment_confirmed(tx: &mut SqliteConnection, payment_id: i64) -> AppResult<()> {
    Err(AppError::internal("not implemented: records::api::on_payment_confirmed"))
}
