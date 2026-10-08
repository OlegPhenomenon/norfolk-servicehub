//! Staff view of the response letters a case's workflow waits for (`documents.letter_issued:<type>` steps).
//! Letters are issued through `POST /api/cases/{id}/letters` (`decisions::letter` → `api::issue_letter`), only at
//! the current letter step; each letter is linked to the step run it was issued at and satisfies only that run.
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
    /// Workflow step the letter was issued at; `None` for legacy letters issued before step runs were recorded.
    step_key: Option<String>,
    #[serde(skip)]
    current_run: bool,
}

pub async fn list(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut c = state.db.acquire().await?;
    let (case, access) = authz::require_staff_case(&mut c, &actor, id).await?;
    let issued: Vec<IssuedLetter> = sqlx::query_as(
        "SELECT l.id,l.letter_type,d.title,l.document_id,l.document_version_id,l.issued_at,u.display_name AS issued_by_name, \
         r.step_key,(r.id IS NOT NULL AND r.left_at IS NULL) AS current_run \
         FROM issued_letters l JOIN documents d ON d.id=l.document_id LEFT JOIN users u ON u.id=l.issued_by \
         LEFT JOIN workflow_step_runs r ON r.id=l.step_run_id WHERE l.case_id=? ORDER BY l.id",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    let reached: Vec<String> = sqlx::query_scalar("SELECT DISTINCT step_key FROM workflow_step_runs WHERE case_id=?")
        .bind(id)
        .fetch_all(&mut *c)
        .await?;
    let open = matches!(case.status.as_str(), "submitted" | "in_progress" | "waiting_on_applicant");
    let may_issue = access.can_manage()
        && open
        && actor.roles_for_service(case.service_id).iter().any(|r| super::api::LETTER_ROLES.contains(r));
    let steps: Vec<Value> = super::api::letter_steps(&mut c, &case)
        .await?
        .into_iter()
        .map(|(step, letter_type)| {
            let current = open && case.current_step.as_deref() == Some(step.key.as_str());
            let ours =
                |l: &IssuedLetter| l.letter_type == letter_type && l.step_key.as_deref() == Some(step.key.as_str());
            // The current step counts only a letter of its open run (a reopened step needs a new letter);
            // legacy unlinked letters only document closed cases.
            let done = if current {
                issued.iter().any(|l| ours(l) && l.current_run)
            } else {
                issued.iter().any(|l| ours(l) || (!open && l.step_key.is_none() && l.letter_type == letter_type))
            };
            json!({
                "step_key": step.key,
                "step_label": step.label,
                "letter_type": letter_type,
                "label": super::api::letter_label(&letter_type),
                "current": current,
                "reached": reached.contains(&step.key),
                "issued": done,
                "can_issue": may_issue && current && !done,
            })
        })
        .collect();
    let can_issue = steps.iter().any(|s| s["can_issue"] == true);
    Ok(Json(json!({"revision":case.revision,"can_issue":can_issue,"steps":steps,"issued":issued})))
}
