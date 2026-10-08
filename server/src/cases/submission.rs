use super::{
    core::{self, CaseRow, Visibility},
    record, require_edit,
};
use crate::{
    auth::Actor,
    db, deadlines,
    error::{AppError, AppResult},
    hooks,
    idempotency::{self, IdempotencyKey},
    notify::{self, Notice},
    services::{definition, validation},
    state::AppState,
    time,
    web::{Json, Path},
};
use axum::{Router, extract::State, http::StatusCode, routing::post};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
pub fn routes() -> Router<AppState> {
    Router::new().route("/api/cases/{id}/submit", post(submit))
}
pub async fn submit_case(tx: &mut SqliteConnection, state: &AppState, actor: &Actor, id: i64) -> AppResult<CaseRow> {
    let case = require_edit(tx, actor, id).await?;
    if case.status != "draft" {
        if case.submitted_at.is_some() {
            return Ok(case);
        }
        return Err(AppError::conflict("This draft is no longer available."));
    }
    // Never submit on a retired version: move the draft to the current one; its answers are re-validated below.
    let case = match super::drafts::rebind(tx, actor, &case).await? {
        super::drafts::Pinned::Current => case,
        super::drafts::Pinned::Replaced { .. } => core::load_case(tx, id).await?,
        super::drafts::Pinned::Unavailable => {
            return Err(AppError::conflict(
                "This service is not accepting requests at the moment. Your draft has been kept.",
            ));
        }
    };
    let def = definition::load_for_case(tx, &case).await?;
    let draft: String =
        sqlx::query_scalar("SELECT answers_json FROM case_drafts WHERE case_id=?").bind(id).fetch_one(&mut *tx).await?;
    let answers = validation::validate_answers(tx, &case.module, &def, &serde_json::from_str::<Value>(&draft)?).await?;
    let mut missing = vec![];
    for d in def
        .documents
        .iter()
        .filter(|d| d.required && d.show_if.as_ref().is_none_or(|s| s.matches(answers.get(&s.field))))
    {
        let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM documents d JOIN document_versions v ON v.document_id=d.id JOIN blobs b ON b.id=v.blob_id WHERE d.case_id=? AND d.requirement_key=? AND d.disposed_at IS NULL AND b.scan_status IN ('clean','not_scanned') AND v.version=(SELECT MAX(v2.version) FROM document_versions v2 WHERE v2.document_id=d.id))").bind(id).bind(&d.key).fetch_one(&mut *tx).await?;
        if !exists {
            missing.push((format!("documents.{}", d.key), format!("Upload {} before submitting.", d.label)));
        }
    }
    if !missing.is_empty() {
        return Err(AppError::validation(missing));
    }
    let snapshot: String = sqlx::query_scalar("SELECT definition_json FROM service_versions WHERE id=?")
        .bind(case.service_version_id)
        .fetch_one(&mut *tx)
        .await?;
    let now = time::fmt(state.now());
    let sid:i64=sqlx::query_scalar("INSERT INTO submissions(case_id,service_version_id,definition_snapshot_json,definition_sha256,answers_json,submitted_by,submitted_at) VALUES (?,?,?,?,?,?,?) RETURNING id").bind(id).bind(case.service_version_id).bind(&snapshot).bind(idempotency::request_hash(snapshot.as_bytes())).bind(answers.to_string()).bind(actor.db_id()).bind(&now).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO submission_documents(submission_id,document_version_id,requirement_key) SELECT ?,v.id,d.requirement_key FROM documents d JOIN document_versions v ON v.document_id=d.id WHERE d.case_id=? AND d.disposed_at IS NULL AND v.version=(SELECT MAX(v2.version) FROM document_versions v2 WHERE v2.document_id=d.id)").bind(sid).bind(id).execute(&mut *tx).await?;
    let first = def.workflow.steps.first().ok_or_else(|| AppError::conflict("The service has no workflow."))?;
    let number = core::assign_number(tx, id).await?;
    sqlx::query(
        "UPDATE cases SET status='submitted',current_step=?,submitted_at=?,updated_at=?,revision=revision+1 WHERE id=?",
    )
    .bind(&first.key)
    .bind(&now)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let case = core::load_case(tx, id).await?;
    hooks::on_submit(tx, state, actor, &case, &answers).await?;
    deadlines::api::on_trigger_at(tx, id, "submitted", state.now()).await?;
    super::workflow::enter_step(tx, state, actor, &case, first).await?;
    sqlx::query("DELETE FROM case_drafts WHERE case_id=?").bind(id).execute(&mut *tx).await?;
    core::reindex_search(tx, id).await?;
    record(
        tx,
        actor,
        id,
        "submitted",
        Visibility::Applicant,
        &format!("Request {number} received."),
        json!({"number":number}),
    )
    .await?;
    let body = if case.module == "venue_booking" {
        "We received your request. Booking not confirmed yet."
    } else {
        "We received your request. It is not approved yet."
    };
    notify::send(
        tx,
        Notice {
            user_id: case.applicant_user_id,
            email: case.applicant_email.clone(),
            phone: case.applicant_phone.clone(),
            case_id: Some(id),
            subject: format!("Request {number} received"),
            body: body.into(),
            link: Some(format!("/my/cases/{id}")),
        },
    )
    .await?;
    if first.kind == definition::StepKind::Complete {
        super::workflow::close(tx, state, actor, id, "completed", "Request completed automatically.").await?;
    }
    core::load_case(tx, id).await
}
pub async fn submit_idempotent(state: &AppState, actor: &Actor, id: i64, key: &str) -> AppResult<Value> {
    let mut tx = db::write_tx(&state.db).await?;
    require_edit(&mut tx, actor, id).await?;
    let hash = idempotency::json_hash(&json!({"case_id":id}));
    if let Some(previous) = idempotency::lookup(&mut tx, actor.user_id, "case.submit", key, &hash).await? {
        return Ok(previous.body);
    }
    let case = submit_case(&mut tx, state, actor, id).await?;
    let result = json!(case);
    idempotency::store(&mut tx, actor.user_id, "case.submit", key, &hash, StatusCode::OK, &result).await?;
    tx.commit().await?;
    Ok(result)
}
async fn submit(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    IdempotencyKey(key): IdempotencyKey,
) -> AppResult<Json<Value>> {
    let key = key.ok_or_else(|| AppError::field("idempotency_key", "Send an Idempotency-Key for submission."))?;
    Ok(Json(submit_idempotent(&state, &actor, id, &key).await?))
}
