use crate::{
    auth::Actor,
    cases::core::CaseRow,
    error::{AppError, AppResult},
    finance::api::QuoteLine,
    services::definition::{FieldDef, FieldType, StepDef},
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
    let Some(id) = value.get("decision_id").and_then(Value::as_i64) else {
        return Ok(Some("Choose an issued approval.".into()));
    };
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM decisions WHERE id=? AND status='issued' AND outcome IN ('approved','approved_with_conditions') AND decision_type IN ('development_approval','building_approval','modification_approval'))").bind(id).fetch_one(tx).await?;
    Ok((!valid).then(|| "Choose an issued approval.".into()))
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
pub async fn step_guard(_tx: &mut SqliteConnection, _case: &CaseRow, _step: &StepDef) -> AppResult<Option<String>> {
    Ok(None)
}
pub async fn step_guard_handler(tx: &mut SqliteConnection, case: &CaseRow, handler: &str) -> AppResult<Option<String>> {
    if handler == "documents.exhibition_closed" {
        super::exhibition::close_due(tx, &crate::time::now_str()).await?;
        let states: Vec<String> =
            sqlx::query_scalar("SELECT status FROM exhibitions WHERE case_id=? AND status<>'withdrawn'")
                .bind(case.id)
                .fetch_all(&mut *tx)
                .await?;
        return Ok((states.is_empty()||states.iter().any(|s|s!="closed")).then(||"Close the public exhibition, or skip this optional step with a recorded reason if exhibition is not required.".into()));
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
    _tx: &mut SqliteConnection,
    _case: &CaseRow,
) -> AppResult<Option<(NaiveDate, Vec<QuoteLine>)>> {
    Ok(None)
}
