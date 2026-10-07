//! Pending cross-module contracts. Table ownership forbids implementing these writes in S5.
//! Owners can replace these bridges with calls to their transaction-taking public APIs at merge.
use crate::{
    auth::Actor,
    cases::core::CaseRow,
    error::{AppError, AppResult},
};
use sqlx::SqliteConnection;
fn missing(_owner: &str, operation: &str) -> AppError {
    AppError::conflict(format!("{operation} is temporarily unavailable. Please contact Council."))
}
/// Requested S1: cases::api::assign_owner(tx, actor, case_id, user_id, reason).
pub async fn assign_owner(
    _tx: &mut SqliteConnection,
    _actor: &Actor,
    _case_id: i64,
    _user_id: i64,
    _reason: &str,
) -> AppResult<()> {
    Err(missing("cases", "Assigning a complaints officer"))
}
/// Requested S1: cases::api::create_review(tx, actor, original, reason) -> CaseRow.
/// Must copy frozen submission, assign number, set submitted timestamp/current triage run, link review_of.
pub async fn create_review(
    _tx: &mut SqliteConnection,
    _actor: &Actor,
    _original: &CaseRow,
    _reason: &str,
) -> AppResult<CaseRow> {
    Err(missing("cases", "Creating a complaint review"))
}
/// Requested platform: cases::core::set_import_dates(tx, case_id, submitted_at, closed_at).
pub async fn set_import_dates(
    _tx: &mut SqliteConnection,
    _case_id: i64,
    _submitted: &str,
    _closed: Option<&str>,
) -> AppResult<()> {
    Err(missing("cases", "Importing legacy dates"))
}
/// Requested S2: documents::api::dispose_case_files(tx, case_id) -> Vec<String> (orphan hashes).
/// Retain immutable version/evidence metadata; return only hashes with no live reference.
pub async fn dispose_documents(_tx: &mut SqliteConnection, _case_id: i64) -> AppResult<Vec<String>> {
    Err(missing("documents", "Disposing document files"))
}
/// Requested platform: auth::users::deactivate_user(tx, user_id). Revoke live sessions immediately.
pub async fn deactivate_user(_tx: &mut SqliteConnection, _id: i64) -> AppResult<()> {
    Err(missing("authentication", "Deactivating a user"))
}
