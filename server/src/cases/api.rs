//! Transactional commands for trusted cross-module callers. HTTP authorization stays at the boundary.
use super::core::{self, CaseRow, NewCase, Visibility};
use crate::{
    auth::Actor,
    error::{AppError, AppResult},
    time,
};
use serde_json::json;
use sqlx::SqliteConnection;

pub async fn assign_owner(
    tx: &mut SqliteConnection,
    actor: &Actor,
    case_id: i64,
    user_id: i64,
    reason: &str,
) -> AppResult<()> {
    super::assignment::add(tx, actor, case_id, user_id, "owner", reason, true).await
}

/// Start a review without submission hooks: records must copy exclusions before assigning anyone.
pub async fn create_review(
    tx: &mut SqliteConnection,
    actor: &Actor,
    original: &CaseRow,
    reason: &str,
) -> AppResult<CaseRow> {
    if original.module != "complaint" || original.status != "completed" {
        return Err(AppError::conflict("Only completed complaints can be reviewed."));
    }
    let def = crate::services::definition::load_for_case(tx, original).await?;
    let first = def
        .workflow
        .steps
        .first()
        .filter(|s| s.key == "triage" && s.kind == crate::services::definition::StepKind::Review)
        .ok_or_else(|| AppError::conflict("The complaint definition needs a triage step."))?;
    let review = core::create_case(
        tx,
        NewCase {
            service_id: original.service_id,
            service_version_id: original.service_version_id,
            module: original.module.clone(),
            title: format!("Review of {}", original.number.as_deref().unwrap_or("feedback")),
            status: "submitted".into(),
            applicant_user_id: original.applicant_user_id,
            applicant_org_id: original.applicant_org_id,
            applicant_name: original.applicant_name.clone(),
            applicant_email: original.applicant_email.clone(),
            applicant_phone: original.applicant_phone.clone(),
            intake_channel: "online".into(),
            recorded_by_user_id: actor.db_id(),
            property_ref: original.property_ref.clone(),
        },
    )
    .await?;
    let now = time::now_str();
    let number = core::assign_number(tx, review.id).await?;
    sqlx::query("UPDATE cases SET submitted_at=?,current_step=? WHERE id=?")
        .bind(&now)
        .bind(&first.key)
        .bind(review.id)
        .execute(&mut *tx)
        .await?;
    let sid: i64 = sqlx::query_scalar("INSERT INTO submissions(case_id,service_version_id,definition_snapshot_json,definition_sha256,answers_json,submitted_by,submitted_at) SELECT ?,service_version_id,definition_snapshot_json,definition_sha256,answers_json,?,? FROM submissions WHERE case_id=? RETURNING id")
        .bind(review.id).bind(actor.db_id()).bind(&now).bind(original.id).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO submission_documents(submission_id,document_version_id,requirement_key) SELECT ?,sd.document_version_id,sd.requirement_key FROM submission_documents sd JOIN submissions s ON s.id=sd.submission_id WHERE s.case_id=?")
        .bind(sid).bind(original.id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO workflow_step_runs(case_id,step_key,entered_at) VALUES(?,?,?)")
        .bind(review.id)
        .bind(&first.key)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO case_links(from_case_id,to_case_id,kind,note,created_by,created_at) VALUES(?,?,'review_of',?,?,?)",
    )
    .bind(review.id)
    .bind(original.id)
    .bind(reason)
    .bind(actor.db_id())
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    crate::deadlines::api::on_trigger(tx, review.id, "submitted").await?;
    crate::deadlines::api::on_trigger(tx, review.id, "step:triage").await?;
    core::reindex_search(tx, review.id).await?;
    super::record(
        tx,
        actor,
        review.id,
        "submitted",
        Visibility::Applicant,
        &format!("Review {number} received."),
        json!({"review_of":original.id}),
    )
    .await?;
    core::load_case(tx, review.id).await
}
