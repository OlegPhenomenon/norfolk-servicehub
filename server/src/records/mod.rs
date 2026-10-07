// OWNER: records
#![allow(dead_code, unused_variables)]
//! Complaints, manager dashboard, retention & legal hold, integrations outbox, legacy import, case export, admin.

pub mod api;
pub mod backup;
pub mod hooks;

use axum::Router;
use serde_json::Value;
use sqlx::SqliteConnection;

use crate::error::AppResult;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
}

/// Jobs with kind prefix `integration.` or `records.`.
pub async fn handle_job(state: &AppState, kind: &str, payload: &Value) -> AppResult<()> {
    Ok(())
}

/// Seeds external systems, retention rules, … (inside the demo seed transaction).
pub async fn seed(tx: &mut SqliteConnection, state: &AppState) -> AppResult<()> {
    Ok(())
}
