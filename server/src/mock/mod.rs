//! Mock external services built into the binary (`/mock/**`), clearly branded "test mode".
//! Server-to-server APIs require the `X-Mock-Key` header ([`require_mock_key`]).

pub mod ai;
pub mod mail;
pub mod pay;
pub mod records;

use axum::Router;
use axum::http::HeaderMap;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::web::ct_eq;

pub fn routes() -> Router<AppState> {
    Router::new().merge(mail::routes()).merge(pay::routes()).merge(records::routes()).merge(ai::routes())
}

/// `unauthorized` unless `X-Mock-Key` equals `MOCK_API_KEY`.
pub fn require_mock_key(state: &AppState, headers: &HeaderMap) -> AppResult<()> {
    let sent = headers.get("x-mock-key").and_then(|v| v.to_str().ok()).unwrap_or("");
    if ct_eq(sent, &state.cfg.mock_api_key) { Ok(()) } else { Err(AppError::unauthorized()) }
}
