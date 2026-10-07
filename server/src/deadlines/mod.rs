// OWNER: services
#![allow(dead_code, unused_variables)]
//! Deadline engine (business/calendar days, pauses, breaches).

pub mod api;

use axum::Router;
use serde_json::Value;

use crate::error::AppResult;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
}

/// Jobs with kind prefix `deadline.` (e.g. `deadline.sweep`, enqueued every minute by the scheduler).
pub async fn handle_job(state: &AppState, kind: &str, payload: &Value) -> AppResult<()> {
    Ok(())
}
