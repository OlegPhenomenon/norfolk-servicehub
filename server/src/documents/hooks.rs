use crate::{
    auth::Actor,
    cases::core::CaseRow,
    error::{AppError, AppResult},
    finance::api::QuoteLine,
    services::definition::{FieldDef, FieldType, StepDef, StepKind},
    state::AppState,
};
use chrono::NaiveDate;
use serde_json::Value;
use sqlx::SqliteConnection;
pub async fn validate_field(
    tx: &mut SqliteConnection,
    _module: &str,
    field: &FieldDef,
    value: &Value,
) -> AppResult<Option<String>> {
    if field.field_type != FieldType::DecisionRef {
        return Ok(None);
    }
    let Some(ids) = super::building::decision_ids(value).filter(|ids| !ids.is_empty()) else {
        return Ok(Some("Choose an issued approval.".into()));
    };
    if ids.len() > 1 && field.extra.get("multiple") != Some(&Value::Bool(true)) {
        return Ok(Some("Choose one issued approval.".into()));
    }
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != ids.len() {
        return Ok(Some("Choose each approval only once.".into()));
    }
    for id in ids {
        let valid: bool = sqlx::query_scalar(&format!(
            "SELECT EXISTS(SELECT 1 FROM decisions d WHERE d.id=? AND {})",
            super::building::CURRENT_APPROVAL
        ))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if !valid {
            return Ok(Some("Choose an issued approval that is still current.".into()));
        }
    }
    Ok(None)
}
pub async fn on_submit(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: &CaseRow,
    answers: &Value,
) -> AppResult<()> {
    if case.module == "building" {
        super::building::on_submit(tx, state, actor, case, answers).await?;
    }
    Ok(())
}
pub async fn on_step_entered(
    _tx: &mut SqliteConnection,
    _state: &AppState,
    _actor: &Actor,
    _case: &CaseRow,
    _step: &StepDef,
    _step_run_id: i64,
) -> AppResult<()> {
    Ok(())
}
pub async fn step_guard(tx: &mut SqliteConnection, case: &CaseRow, step: &StepDef) -> AppResult<Option<String>> {
    if step.kind == StepKind::Payment {
        return crate::finance::building_fees::payment_block(tx, case).await;
    }
    Ok(None)
}
/// A skip never leaves an open exhibition or unconsidered comments behind; a skipped (legacy optional)
/// exhibition step is recorded as "not required" with the skip reason.
pub async fn on_skip(
    tx: &mut SqliteConnection,
    actor: &Actor,
    case: &CaseRow,
    step: &StepDef,
    reason: &str,
) -> AppResult<Option<String>> {
    if step.handler.as_deref() == Some("documents.exhibition_closed") {
        return super::exhibition::on_skip(tx, actor.db_id(), case.id, reason).await;
    }
    Ok(None)
}
pub async fn step_guard_handler(tx: &mut SqliteConnection, case: &CaseRow, handler: &str) -> AppResult<Option<String>> {
    if handler == "documents.exhibition_closed" {
        return super::exhibition::step_block(tx, case.id).await;
    }
    if let Some(t) = handler.strip_prefix("documents.letter_issued:") {
        if !matches!(t, "road_response" | "complaint_response") {
            return Err(AppError::internal("Unknown letter handler."));
        }
        return Ok((!super::api::letter_issued(tx, case.id, t).await?)
            .then(|| "Issue the response letter before continuing.".into()));
    }
    Err(AppError::internal(format!("Unknown documents handler: {handler}")))
}
pub async fn pricing_lines(
    tx: &mut SqliteConnection,
    case: &CaseRow,
) -> AppResult<Option<(NaiveDate, Vec<QuoteLine>)>> {
    crate::finance::building_fees::pricing_lines(tx, case).await
}
