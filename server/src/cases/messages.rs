// OWNER: services
#![allow(dead_code, unused_variables)]
//! Applicant ↔ staff conversation, internal notes and case links.

use axum::Router;
use sqlx::SqliteConnection;

use crate::auth::Actor;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// `/api/cases/{id}/messages`, `/api/staff/cases/{id}/notes`, … .
pub fn routes() -> Router<AppState> {
    Router::new()
}

/// Posts a staff message to the applicant (event + notification). With `requires_response = true`
/// the applicant's deadline clock may pause (`deadlines::api::pause_for_applicant`). Returns the message id.
pub async fn post_staff_message(
    tx: &mut SqliteConnection,
    actor: &Actor,
    case_id: i64,
    body: &str,
    document_version_id: Option<i64>,
    requires_response: bool,
) -> AppResult<i64> {
    Err(AppError::internal("not implemented: cases::messages::post_staff_message"))
}
