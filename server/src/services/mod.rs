// OWNER: services
#![allow(dead_code, unused_variables)]
//! Service catalog, definitions, validation, builder, bulk import, mock-AI suggestion.

pub mod definition;

use axum::Router;
use sqlx::SqliteConnection;

use crate::error::AppResult;
use crate::state::AppState;

/// `/api/services/**`, `/api/admin/services/**`, … (catalog and builder).
pub fn routes() -> Router<AppState> {
    Router::new()
}

/// Seeds the demo service catalog (inside the demo seed transaction; called after platform seed data).
pub async fn seed(tx: &mut SqliteConnection, state: &AppState) -> AppResult<()> {
    Ok(())
}
