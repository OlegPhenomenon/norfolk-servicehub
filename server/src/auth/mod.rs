//! Authentication: passwords, sessions, staff TOTP, extractors and demo persona login.
//!
//! Handlers pick an extractor from [`extract`]: `Actor` (any signed-in user; staff must have passed
//! TOTP), `StaffActor`, `OptionalActor`, `AuthSession` (TOTP not yet required).

pub mod actor;
pub mod demo;
pub mod extract;
pub mod password;
pub mod routes;
pub mod session;
pub mod throttle;
pub mod totp;
pub mod users;

#[cfg(test)]
mod tests;

pub use actor::{Actor, RoleGrant, SYSTEM_USER_ID, UserKind};
pub use extract::{AuthSession, MaybeSession, OptionalActor, StaffActor};

use axum::Router;

use crate::state::AppState;

/// `/api/me`, `/api/auth/**`, `/api/notifications/**`, `/api/demo/**`.
pub fn routes() -> Router<AppState> {
    Router::new().merge(routes::routes()).merge(demo::routes())
}
