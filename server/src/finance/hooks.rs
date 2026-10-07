use crate::{
    cases::core::CaseRow,
    error::{AppError, AppResult},
};
use sqlx::SqliteConnection;
pub async fn step_guard_handler(tx: &mut SqliteConnection, case: &CaseRow, handler: &str) -> AppResult<Option<String>> {
    if handler != "finance.deposits_settled" {
        return Err(AppError::validation_msg("Unknown finance workflow handler."));
    }
    Ok((!super::api::deposits_settled(tx, case.id).await?)
        .then(|| "A bond decision and confirmed refund are still required.".into()))
}
