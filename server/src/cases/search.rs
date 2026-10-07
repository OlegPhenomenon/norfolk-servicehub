// OWNER: services
#![allow(dead_code, unused_variables)]
//! Case listings and full-text search (use `authz::case_scope_sql`).

use axum::Router;

use crate::state::AppState;

/// HTTP routes of this file (full paths, e.g. `/api/...`). Merged by the parent module.
pub fn routes() -> Router<AppState> {
    Router::new()
}
