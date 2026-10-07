// OWNER: services
#![allow(dead_code, unused_variables)]
//! Workflow engine: advance, skip optional steps, request info, refuse, close, reopen.
//!
//! On entering a step call `hooks::on_step_entered` (it issues the invoice for `payment` steps and creates
//! the task for `task` steps — do not call `finance`/`operations` for that yourself). To leave a step call
//! `hooks::step_guard` (it evaluates every step kind; `Some(reason)` blocks).
//!
//! Signature note: `advance`, `try_auto_advance` and `close` take `&AppState` because entering the next step
//! runs `hooks::on_step_entered`, which needs it. Automatic advances use `Actor::system()`.

use axum::Router;
use sqlx::SqliteConnection;

use crate::auth::Actor;
use crate::cases::core::CaseRow;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// `/api/staff/cases/{id}/advance`, … .
pub fn routes() -> Router<AppState> {
    Router::new()
}

/// Staff advance of the current step (checks role, `expected_revision`, `hooks::step_guard`).
pub async fn advance(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case_id: i64,
    expected_revision: i64,
    note: Option<String>,
) -> AppResult<CaseRow> {
    Err(AppError::internal("not implemented: cases::workflow::advance"))
}

/// System advance if the current step's guard passes (called by owners after webhooks, task completion,
/// refund confirmation …). No-op when the guard still blocks.
pub async fn try_auto_advance(tx: &mut SqliteConnection, state: &AppState, case_id: i64) -> AppResult<()> {
    Err(AppError::internal("not implemented: cases::workflow::try_auto_advance"))
}

/// Closes a case with `outcome` (`completed`, `refused`, `withdrawn`, `cancelled`, `closed_duplicate`);
/// calls `records::api::on_case_closed`.
pub async fn close(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case_id: i64,
    outcome: &str,
    reason: &str,
) -> AppResult<()> {
    Err(AppError::internal("not implemented: cases::workflow::close"))
}
