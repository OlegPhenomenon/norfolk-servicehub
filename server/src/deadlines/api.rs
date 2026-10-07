// OWNER: services
#![allow(dead_code, unused_variables)]
//! Cross-module deadline API.

use sqlx::SqliteConnection;

use crate::error::{AppError, AppResult};

/// Pauses the case's pausable running deadlines while waiting for the applicant's answer to `message_id`.
pub async fn pause_for_applicant(
    tx: &mut SqliteConnection,
    case_id: i64,
    message_id: i64,
    reason: &str,
) -> AppResult<()> {
    Err(AppError::internal("not implemented: deadlines::api::pause_for_applicant"))
}

/// Resumes paused deadlines (`why`: `applicant_responded` | `staff_resumed` | `cap_reached`).
pub async fn resume(tx: &mut SqliteConnection, case_id: i64, why: &str) -> AppResult<()> {
    Err(AppError::internal("not implemented: deadlines::api::resume"))
}

/// Starts/meets deadlines for a trigger: `"submitted"`, `"step:<key>"`, `"decision_issued"`, `"closed"`.
pub async fn on_trigger(tx: &mut SqliteConnection, case_id: i64, trigger: &str) -> AppResult<()> {
    Err(AppError::internal("not implemented: deadlines::api::on_trigger"))
}
