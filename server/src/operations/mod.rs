// OWNER: operations
#![allow(dead_code, unused_variables)]
//! Resources, bookings & calendar, equipment hire & usage, field tasks & offline sync, road issues.

pub mod api;
pub mod hooks;

use axum::Router;
use serde_json::Value;
use sqlx::SqliteConnection;

use crate::error::AppResult;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
}

/// Jobs with kind prefix `ops.`.
pub async fn handle_job(state: &AppState, kind: &str, payload: &Value) -> AppResult<()> {
    Ok(())
}

/// Seeds resources, bookable units, … (inside the demo seed transaction).
pub async fn seed(tx: &mut SqliteConnection, state: &AppState) -> AppResult<()> {
    Ok(())
}
