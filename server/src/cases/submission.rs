// OWNER: services
#![allow(dead_code, unused_variables)]
//! Submission: validate answers against the frozen definition, snapshot, number, start workflow.

use axum::Router;

use crate::state::AppState;

/// HTTP routes of this file (full paths, e.g. `/api/...`). Merged by the parent module.
pub fn routes() -> Router<AppState> {
    Router::new()
}
