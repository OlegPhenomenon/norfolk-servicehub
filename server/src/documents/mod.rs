// OWNER: documents
#![allow(dead_code, unused_variables)]
//! Documents & versions, comments, decisions & templates, building projects, planning certificates, exhibitions & redaction.

pub mod api;
pub mod hooks;

use axum::Router;
use sqlx::SqliteConnection;

use crate::error::AppResult;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
}

/// Seeds decision templates etc. (inside the demo seed transaction).
pub async fn seed(tx: &mut SqliteConnection, state: &AppState) -> AppResult<()> {
    Ok(())
}
