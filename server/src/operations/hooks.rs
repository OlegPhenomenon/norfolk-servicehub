// OWNER: operations
#![allow(dead_code, unused_variables)]
//! Module hooks for `venue_booking`, `equipment_hire` and `road_issue` cases and `operations.*` step handlers. Called only through `crate::hooks` (dispatch by `cases.module` / step handler).

use chrono::NaiveDate;
use serde_json::Value;
use sqlx::SqliteConnection;

use crate::auth::Actor;
use crate::cases::core::CaseRow;
use crate::error::AppResult;
use crate::finance::api::QuoteLine;
use crate::services::definition::{FieldDef, StepDef};
use crate::state::AppState;

/// Validates a module-specific field value (`module` = the case's module). `Some(message)` = invalid.
pub async fn validate_field(
    tx: &mut SqliteConnection,
    module: &str,
    field: &FieldDef,
    value: &Value,
) -> AppResult<Option<String>> {
    Ok(None)
}

/// Runs inside the submission transaction after the case is numbered and the snapshot stored.
pub async fn on_submit(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: &CaseRow,
    answers: &Value,
) -> AppResult<()> {
    Ok(())
}

/// Runs when the case enters `step` (after the generic payment/task handling in `crate::hooks`).
pub async fn on_step_entered(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: &CaseRow,
    step: &StepDef,
    step_run_id: i64,
) -> AppResult<()> {
    Ok(())
}

/// Extra module guard for non-`module` steps (after the generic guard passed). `Some(reason)` blocks.
pub async fn step_guard(tx: &mut SqliteConnection, case: &CaseRow, step: &StepDef) -> AppResult<Option<String>> {
    Ok(None)
}

/// Guard of a `module` step whose `handler` starts with this module's prefix
/// (e.g. `operations.booking_confirmed`). `Some(reason)` blocks; unknown handler → `Err`.
pub async fn step_guard_handler(tx: &mut SqliteConnection, case: &CaseRow, handler: &str) -> AppResult<Option<String>> {
    Ok(None)
}

/// Module-specific pricing: `Some((pricing_date, lines))`, or `None` to use the generic definition pricing.
pub async fn pricing_lines(
    tx: &mut SqliteConnection,
    case: &CaseRow,
) -> AppResult<Option<(NaiveDate, Vec<QuoteLine>)>> {
    Ok(None)
}
