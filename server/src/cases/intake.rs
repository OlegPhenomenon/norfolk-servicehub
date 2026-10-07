// OWNER: services
#![allow(dead_code, unused_variables)]
//! Assisted intake: staff record phone / walk-in / email / post requests.

use axum::Router;

use crate::state::AppState;

/// HTTP routes of this file (full paths, e.g. `/api/...`). Merged by the parent module.
pub fn routes() -> Router<AppState> {
    Router::new()
}
