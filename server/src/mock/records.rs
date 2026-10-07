// OWNER: records
#![allow(dead_code, unused_variables)]
//! Mock records systems (Content Manager, Civica Altitude) at `/mock/records/**`.

use axum::Router;

use crate::state::AppState;

/// HTTP routes of this file (full paths, e.g. `/api/...`). Merged by the parent module.
pub fn routes() -> Router<AppState> {
    Router::new()
}
