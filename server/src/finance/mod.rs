//! Prices, receipts, allocations, deposits and a balanced integer ledger.
pub mod api;
pub mod building_fees;
mod deposits;
pub mod hooks;
pub(crate) mod ledger;
mod payments;
mod prices;
mod routes;
mod statements;
#[cfg(test)]
mod tests;
mod views;
mod webhooks;
use crate::{
    error::{AppError, AppResult},
    state::AppState,
};
use serde_json::Value;
use sqlx::SqliteConnection;
pub fn routes() -> axum::Router<AppState> {
    routes::routes()
}
pub async fn seed(tx: &mut SqliteConnection, state: &AppState) -> AppResult<()> {
    prices::seed(tx, state).await?;
    building_fees::seed_scale(tx).await
}
pub async fn handle_job(state: &AppState, kind: &str, payload: &Value) -> AppResult<()> {
    match kind {
        "finance.request_refund" => {
            deposits::request_job(
                state,
                payload["refund_id"].as_i64().ok_or_else(|| AppError::internal("Missing refund id"))?,
            )
            .await
        }
        "finance.mock_refund" | "finance.mock_webhook" => crate::mock::pay::handle_job(state, kind, payload).await,
        _ => Err(AppError::internal(format!("Unknown finance job {kind}"))),
    }
}
