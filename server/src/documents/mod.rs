//! Immutable documents, decisions, building projects and safe public exhibitions.
pub mod api;
pub mod building;
pub mod decisions;
pub mod exhibition;
pub mod hooks;
mod letters;
mod seeds;
#[cfg(test)]
mod tests;
mod uploads;

use crate::{
    auth::Actor,
    authz::{self, CaseAccess, Role},
    cases::core::{CaseRow, Visibility},
    error::{AppError, AppResult},
    state::AppState,
};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post, put},
};
use sqlx::SqliteConnection;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cases/{id}/documents", get(uploads::list).post(uploads::upload))
        .route("/api/documents/{id}/versions", post(uploads::version))
        .route("/api/document-versions/{id}/download", get(uploads::download))
        .route("/api/document-versions/{id}/comments", post(uploads::comment))
        .route("/api/cases/{id}/decisions", get(decisions::list).post(decisions::create))
        .route("/api/cases/{id}/decisions/{did}", put(decisions::update))
        .route("/api/cases/{id}/decisions/{did}/{action}", post(decisions::action))
        .route("/api/cases/{id}/letters", get(letters::list).post(decisions::letter))
        .route("/api/decision-templates", get(decisions::templates))
        .route("/api/my/issued-approvals", get(building::approvals))
        .route("/api/my/building-projects", get(building::my_projects))
        .route("/api/building-projects/{id}", get(building::detail))
        .route("/api/cases/{id}/building-route", get(building::route))
        .route("/api/cases/{id}/approval-scope", post(building::set_scope))
        .merge(exhibition::routes())
        .layer(DefaultBodyLimit::max(crate::storage::MAX_BYTES + 1024 * 1024))
}

pub async fn seed(tx: &mut SqliteConnection, state: &AppState) -> AppResult<()> {
    seeds::seed(tx, state).await
}

pub(crate) async fn access(tx: &mut SqliteConnection, actor: &Actor, id: i64) -> AppResult<(CaseRow, CaseAccess)> {
    let (case, a) = authz::require_case(tx, actor, id).await?;
    if a == CaseAccess::TaskOnly {
        return Err(AppError::not_found());
    }
    Ok((case, a))
}
pub(crate) async fn manage(tx: &mut SqliteConnection, actor: &Actor, id: i64, roles: &[Role]) -> AppResult<CaseRow> {
    let (case, a) = authz::require_staff_case(tx, actor, id).await?;
    if !a.can_manage() || !actor.roles_for_service(case.service_id).iter().any(|r| roles.contains(r)) {
        return Err(AppError::forbidden());
    }
    Ok(case)
}
pub(crate) fn text(field: &str, value: &str, max: usize) -> AppResult<()> {
    if value.trim().is_empty() || value.chars().count() > max {
        return Err(AppError::field(field, format!("Enter between 1 and {max} characters.")));
    }
    Ok(())
}
pub(crate) async fn changed(
    tx: &mut SqliteConnection,
    actor: Option<i64>,
    case: i64,
    kind: &str,
    visibility: Visibility,
    summary: &str,
) -> AppResult<()> {
    crate::audit::record(tx, actor, kind, "case", Some(case), serde_json::json!({"summary":summary})).await?;
    crate::cases::core::append_event(tx, case, actor, kind, visibility, summary, serde_json::json!({})).await?;
    Ok(())
}
pub(crate) async fn notify_applicant(
    tx: &mut SqliteConnection,
    case: &CaseRow,
    subject: &str,
    body: &str,
) -> AppResult<()> {
    crate::notify::send(
        tx,
        crate::notify::Notice {
            user_id: case.applicant_user_id,
            email: case.applicant_email.clone(),
            phone: case.applicant_user_id.is_none().then(|| case.applicant_phone.clone()).flatten(),
            case_id: Some(case.id),
            subject: subject.into(),
            body: body.into(),
            link: Some(format!("/my/cases/{}?tab=documents.documents", case.id)),
        },
    )
    .await
}
