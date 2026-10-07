// OWNER: services
#![allow(dead_code, unused_variables)]
//! Drafts: create a case from a service, save answers, upload documents before submission.

use axum::Router;

use crate::state::AppState;

/// HTTP routes of this file (full paths, e.g. `/api/...`). Merged by the parent module.
pub fn routes() -> Router<AppState> {
    Router::new()
}
