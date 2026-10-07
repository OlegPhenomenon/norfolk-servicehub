// OWNER: finance
#![allow(dead_code, unused_variables)]
//! Cross-module finance API. Money is integer cents (AUD).

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqliteConnection;

use crate::auth::Actor;
use crate::cases::core::CaseRow;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// A priced line, ready to become an `invoice_lines` row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuoteLine {
    pub price_item_id: Option<i64>,
    pub price_version_id: Option<i64>,
    pub item_code: String,
    /// `fee` | `deposit`.
    pub kind: String,
    pub description: String,
    /// 1000 = 1 unit.
    pub quantity_milli: i64,
    /// Hourly lines: exact minutes (amount = round_half_up(minutes × rate / 60)).
    pub quantity_minutes: Option<i64>,
    pub unit_amount_cents: i64,
    pub amount_cents: i64,
    /// Inputs used, e.g. `{"billable_minutes":330,"source":"equipment_usage#3"}`.
    pub calc: Value,
}

/// Money position of a case.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoneySummary {
    pub invoiced_cents: i64,
    pub credited_cents: i64,
    pub paid_cents: i64,
    pub outstanding_cents: i64,
    pub deposits_held_cents: i64,
    pub refunded_cents: i64,
    pub settled: bool,
}

/// Prices `item_code` for `pricing_date` (effective-dated `price_versions`).
pub async fn quote(
    tx: &mut SqliteConnection,
    item_code: &str,
    quantity_milli: i64,
    quantity_minutes: Option<i64>,
    pricing_date: NaiveDate,
) -> AppResult<QuoteLine> {
    Err(AppError::internal("not implemented: finance::api::quote"))
}

/// Issues an invoice (`kind` = `estimate` | `invoice` | `credit_note`). Returns the invoice id.
#[allow(clippy::too_many_arguments)]
pub async fn issue_invoice(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case_id: i64,
    kind: &str,
    pricing_date: NaiveDate,
    lines: Vec<QuoteLine>,
    basis_note: Option<String>,
) -> AppResult<i64> {
    Err(AppError::internal("not implemented: finance::api::issue_invoice"))
}

/// On entering a payment step: issues the invoice from `hooks::pricing_lines`; no-op (returns `None`)
/// if an unpaid invoice already exists.
pub async fn ensure_invoice_for_step(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: &CaseRow,
) -> AppResult<Option<i64>> {
    Err(AppError::internal("not implemented: finance::api::ensure_invoice_for_step"))
}

/// Generic services: lines from the frozen definition's `pricing` (priced at the submission date).
pub async fn definition_pricing_lines(tx: &mut SqliteConnection, case: &CaseRow) -> AppResult<Vec<QuoteLine>> {
    Err(AppError::internal("not implemented: finance::api::definition_pricing_lines"))
}

/// Reschedule: credit note + new invoice for the difference at the new `pricing_date`.
#[allow(clippy::too_many_arguments)]
pub async fn reprice_case(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case_id: i64,
    pricing_date: NaiveDate,
    new_lines: Vec<QuoteLine>,
    note: &str,
) -> AppResult<()> {
    Err(AppError::internal("not implemented: finance::api::reprice_case"))
}

/// Every issued invoice of the case is settled (ARCHITECTURE §3 Settlement).
pub async fn case_settled(tx: &mut SqliteConnection, case_id: i64) -> AppResult<bool> {
    Err(AppError::internal("not implemented: finance::api::case_settled"))
}

/// Every deposit has a decision and its refund is completed (or fully retained).
pub async fn deposits_settled(tx: &mut SqliteConnection, case_id: i64) -> AppResult<bool> {
    Err(AppError::internal("not implemented: finance::api::deposits_settled"))
}

/// Money position for case pages and dashboards.
pub async fn case_money_summary(tx: &mut SqliteConnection, case_id: i64) -> AppResult<MoneySummary> {
    Err(AppError::internal("not implemented: finance::api::case_money_summary"))
}
