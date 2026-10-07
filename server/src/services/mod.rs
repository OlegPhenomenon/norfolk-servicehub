//! Service catalogue, builder, bulk imports and optional mock AI draft suggestions.
pub mod admin;
pub mod ai_suggest;
pub mod catalog;
pub mod definition;
pub mod imports;
mod seed;
pub mod validation;
use crate::state::AppState;
use axum::Router;
pub use seed::seed;
pub fn routes() -> Router<AppState> {
    Router::new().merge(catalog::routes()).merge(admin::routes()).merge(imports::routes()).merge(ai_suggest::routes())
}

#[cfg(test)]
mod tests;
pub mod upload;
