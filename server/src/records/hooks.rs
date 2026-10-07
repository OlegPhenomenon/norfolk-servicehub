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
    tx: &mut SqliteConnection,
    _module: &str,
    field: &FieldDef,
    value: &Value,
) -> AppResult<Option<String>> {
    if field.key == "staff_member_concerned" {
        let uid = value.as_str().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
        let valid: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=? AND kind='staff' AND is_active=1)")
                .bind(uid)
                .fetch_one(tx)
                .await?;
        return Ok((!valid).then(|| "Choose an active staff member.".into()));
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
    sqlx::query("UPDATE cases SET confidential=1 WHERE id=?").bind(case.id).execute(&mut *tx).await?;
    if let Some(uid) = answers["staff_member_concerned"].as_str().and_then(|v| v.parse::<i64>().ok()) {
        let valid: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=? AND kind='staff' AND is_active=1)")
                .bind(uid)
                .fetch_one(&mut *tx)
                .await?;
        if !valid {
            return Err(AppError::field("staff_member_concerned", "Choose an active staff member."));
        }
        sqlx::query("INSERT INTO complaint_subjects(case_id,staff_user_id) VALUES(?,?)")
            .bind(case.id)
            .bind(uid)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO case_access_denials(case_id,user_id,reason,created_by,created_at) VALUES(?,?,'Subject of complaint',?,?)").bind(case.id).bind(uid).bind(actor.db_id()).bind(crate::time::fmt(state.now())).execute(&mut *tx).await?;
    }
    let owner = super::complaints::select_handler(tx, case.id, None).await?;
    let Some(owner) = owner else {
        return Err(AppError::conflict("No eligible complaints officer or manager is available."));
    };
    crate::cases::api::assign_owner(
        state,
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
