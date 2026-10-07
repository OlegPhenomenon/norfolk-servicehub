//! Complaint hooks, dispatched by the platform using the frozen module.
use crate::{
    auth::Actor,
    cases::core::CaseRow,
    error::{AppError, AppResult},
    finance::api::QuoteLine,
    services::definition::{FieldDef, StepDef},
    state::AppState,
};
use chrono::NaiveDate;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
pub async fn validate_field(
    _tx: &mut SqliteConnection,
    _module: &str,
    _field: &FieldDef,
    _value: &Value,
) -> AppResult<Option<String>> {
    Ok(None)
}
pub async fn on_submit(
    tx: &mut SqliteConnection,
    _state: &AppState,
    actor: &Actor,
    case: &CaseRow,
    _answers: &Value,
) -> AppResult<()> {
    sqlx::query("UPDATE cases SET confidential=1 WHERE id=?").bind(case.id).execute(&mut *tx).await?;
    let owner = super::complaints::select_handler(tx, case.id, None).await?;
    let Some(owner) = owner else {
        return Err(AppError::conflict("No eligible complaints officer or manager is available."));
    };
    crate::cases::api::assign_owner(
        tx,
        actor,
        case.id,
        owner,
        "Confidential complaint assigned for independent handling.",
    )
    .await?;
    super::common::changed(
        tx,
        actor,
        case.id,
        "complaint.confidential",
        "Feedback assigned to a complaints officer.",
        json!({"owner_user_id":owner}),
    )
    .await?;
    super::complaints::notify_handler(tx, case, owner).await
}
pub async fn on_step_entered(
    _tx: &mut SqliteConnection,
    _state: &AppState,
    _actor: &Actor,
    _case: &CaseRow,
    _step: &StepDef,
    _run: i64,
) -> AppResult<()> {
    Ok(())
}
pub async fn step_guard(_tx: &mut SqliteConnection, _case: &CaseRow, _step: &StepDef) -> AppResult<Option<String>> {
    Ok(None)
}
pub async fn step_guard_handler(tx: &mut SqliteConnection, case: &CaseRow, handler: &str) -> AppResult<Option<String>> {
    if handler != "records.complaint_response" {
        return Err(AppError::internal(format!("Unknown records handler: {handler}")));
    }
    Ok((!crate::documents::api::letter_issued(tx, case.id, "complaint_response").await?)
        .then(|| "Issue the response letter before completing this complaint.".into()))
}
pub async fn pricing_lines(
    _tx: &mut SqliteConnection,
    _case: &CaseRow,
) -> AppResult<Option<(NaiveDate, Vec<QuoteLine>)>> {
    Ok(None)
}
