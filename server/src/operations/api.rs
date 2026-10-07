// OWNER: operations
#![allow(dead_code, unused_variables)]
//! Cross-module operations API.

use sqlx::SqliteConnection;

use crate::cases::core::CaseRow;
use crate::error::{AppError, AppResult};
use crate::services::definition::StepDef;

/// Creates the field task for a `task` step (kind from `step.task_kind`) linked to `step_run_id`.
/// Called by `hooks::on_step_entered`. `actor` = `users.id` or `None` for system. Returns the task id.
pub async fn create_step_task(
    tx: &mut SqliteConnection,
    case: &CaseRow,
    step: &StepDef,
    step_run_id: i64,
    actor: Option<i64>,
) -> AppResult<i64> {
    Err(AppError::internal("not implemented: operations::api::create_step_task"))
}

/// True when every task of the step run is `done`.
pub async fn step_tasks_done(tx: &mut SqliteConnection, case_id: i64, step_run_id: i64) -> AppResult<bool> {
    Err(AppError::internal("not implemented: operations::api::step_tasks_done"))
}
