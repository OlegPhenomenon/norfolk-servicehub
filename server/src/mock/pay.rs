// OWNER: finance
#![allow(dead_code, unused_variables)]
//! DemoPay: hosted checkout pages and the mock payment provider API (`/mock/pay/**`).

use axum::Router;

use crate::state::AppState;

/// HTTP routes of this file (full paths, e.g. `/api/...`). Merged by the parent module.
pub fn routes() -> Router<AppState> {
    Router::new()
}
