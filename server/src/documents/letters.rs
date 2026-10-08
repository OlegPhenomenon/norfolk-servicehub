//! Staff view of the response letters a case's workflow waits for (`documents.letter_issued:<type>` steps).
//! Letters are issued through `POST /api/cases/{id}/letters` (`decisions::letter` → `api::issue_letter`).
use crate::{
    auth::Actor,
    authz,
    error::AppResult,
    state::AppState,
    web::{Json, Path},
};
use axum::extract::State;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Serialize, sqlx::FromRow)]
struct IssuedLetter {
    id: i64,
    letter_type: String,
    title: String,
    document_id: i64,
    document_version_id: i64,
    issued_at: String,
    issued_by_name: Option<String>,
}

pub async fn list(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut c = state.db.acquire().await?;
    let (case, access) = authz::require_staff_case(&mut c, &actor, id).await?;
    let issued: Vec<IssuedLetter> = sqlx::query_as(
        "SELECT l.id,l.letter_type,d.title,l.document_id,l.document_version_id,l.issued_at,u.display_name AS issued_by_name \
         FROM issued_letters l JOIN documents d ON d.id=l.document_id LEFT JOIN users u ON u.id=l.issued_by \
         WHERE l.case_id=? ORDER BY l.id",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    let steps: Vec<Value> = super::api::letter_steps(&mut c, &case)
        .await?
        .into_iter()
        .map(|(step, letter_type)| {
            json!({
                "step_key": step.key,
                "step_label": step.label,
                "letter_type": letter_type,
                "label": super::api::letter_label(&letter_type),
                "current": case.current_step.as_deref() == Some(step.key.as_str()),
                "issued": issued.iter().any(|l| l.letter_type == letter_type),
            })
        })
        .collect();
    let can_issue = access.can_manage()
        && matches!(case.status.as_str(), "submitted" | "in_progress" | "waiting_on_applicant")
        && actor.roles_for_service(case.service_id).iter().any(|r| super::api::LETTER_ROLES.contains(r));
    Ok(Json(json!({"revision":case.revision,"can_issue":can_issue,"steps":steps,"issued":issued})))
}
