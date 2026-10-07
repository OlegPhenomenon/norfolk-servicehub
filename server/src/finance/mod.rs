// OWNER: finance
#![allow(dead_code, unused_variables)]
//! Price lists, invoices, DemoPay checkout & webhooks, bank statements, allocations, deposits, refunds, ledger.

pub mod api;
pub mod hooks;

use axum::Router;
use serde_json::Value;
use sqlx::SqliteConnection;

use crate::error::AppResult;
use crate::state::AppState;

/// Includes `/api/webhooks/**` (exempt from CSRF; verify the HMAC signature with `cfg.webhook_secret`).
pub fn routes() -> Router<AppState> {
    Router::new()
}

/// Jobs with kind prefix `finance.`.
pub async fn handle_job(state: &AppState, kind: &str, payload: &Value) -> AppResult<()> {
    Ok(())
}

/// Seeds price items and price versions (inside the demo seed transaction).
pub async fn seed(tx: &mut SqliteConnection, state: &AppState) -> AppResult<()> {
    Ok(())
}
