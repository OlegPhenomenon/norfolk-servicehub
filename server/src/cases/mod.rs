//! Cases. `core.rs` is platform-owned (primitives); the other files belong to the services slice.

pub mod api;
pub mod assignment;
pub mod core;
pub mod drafts;
pub mod intake;
pub mod messages;
pub mod search;
pub mod submission;
pub mod timeline;
pub mod workflow;

use axum::Router;

use crate::state::AppState;

/// Merges the routers of every case file.
pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(drafts::routes())
        .merge(submission::routes())
        .merge(workflow::routes())
        .merge(assignment::routes())
        .merge(messages::routes())
        .merge(search::routes())
        .merge(intake::routes())
        .merge(representatives::routes())
}

pub mod representatives;

/// Shared case command audit/event; never used for a public projection.
pub async fn record(
    tx: &mut sqlx::SqliteConnection,
    actor: &crate::auth::Actor,
    id: i64,
    kind: &str,
    visibility: core::Visibility,
    summary: &str,
    data: serde_json::Value,
) -> crate::error::AppResult<()> {
    core::append_event(tx, id, actor.db_id(), kind, visibility, summary, data.clone()).await?;
    crate::audit::record(tx, actor.db_id(), kind, "case", Some(id), data).await
}
/// Who may read, change, delete or submit a draft (and replay its submission). An applicant's own draft belongs
/// to the applicant (or their organisation/representative); staff never act on it. An assisted draft
/// (`recorded_by_user_id` set) is handled only by the staff member who recorded it, through assisted intake.
pub async fn require_edit(
    tx: &mut sqlx::SqliteConnection,
    actor: &crate::auth::Actor,
    id: i64,
) -> crate::error::AppResult<core::CaseRow> {
    let (case, access) = crate::authz::require_case(tx, actor, id).await?;
    match case.recorded_by_user_id {
        None if access == crate::authz::CaseAccess::Applicant => Ok(case),
        Some(recorder) if access.can_manage() && actor.db_id() == Some(recorder) => Ok(case),
        Some(_) if access.is_staff() => Err(crate::error::AppError::forbidden()),
        _ => Err(crate::error::AppError::not_found()),
    }
}
