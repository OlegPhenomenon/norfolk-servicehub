use crate::{
    cases::core::CaseRow,
    error::{AppError, AppResult},
};
use sqlx::SqliteConnection;
pub async fn step_guard_handler(tx: &mut SqliteConnection, case: &CaseRow, handler: &str) -> AppResult<Option<String>> {
    if handler == super::building_fees::HANDLER {
        return Ok(super::building_fees::current(tx, case.id).await?.is_none().then(|| {
            "Record the fee assessment (schedule calculation or staff assessment with its basis) before continuing."
                .into()
        }));
    }
    if handler != "finance.deposits_settled" {
        return Err(AppError::validation_msg("Unknown finance workflow handler."));
    }
    Ok((!super::api::deposits_settled(tx, case.id).await?)
        .then(|| "A bond decision and confirmed refund are still required.".into()))
}
