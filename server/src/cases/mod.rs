//! Cases. `core.rs` is platform-owned (primitives); the other files belong to the services slice.

pub mod assignment;
pub mod core;
pub mod drafts;
pub mod intake;
pub mod messages;
pub mod search;
pub mod submission;
pub mod workflow;

use axum::Router;

use crate::state::AppState;

/// Merges the routers of every case file.
pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(drafts::routes())
        .merge(submission::routes())
        .merge(workflow::routes())
        .merge(assignment::routes())
        .merge(messages::routes())
        .merge(search::routes())
        .merge(intake::routes())
}
