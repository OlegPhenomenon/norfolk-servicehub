//! Session middleware (session lookup + CSRF check) and the auth extractors.
//!
//! Extractors, from weakest to strongest:
//! * [`OptionalActor`] — `Some(actor)` only for a fully authenticated session (residents, or staff who passed TOTP).
//! * [`AuthSession`] — any valid session, TOTP not required (TOTP step, enrolment, logout).
//! * [`Actor`] — valid session; staff must have passed TOTP (else `mfa_required`). Default for resident endpoints.
//! * [`StaffActor`] — like `Actor` and the user is staff (else `forbidden`). Default for `/api/staff/**`.
//!
//! CSRF: every non-GET `/api/**` request (except `/api/webhooks/**` and `/api/public/**`) must send
//! `X-CSRF-Token` equal to the session's token, or — without a session — to the `nsh_csrf` cookie
//! (double-submit). `GET /api/me` returns the right value.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::Method;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::auth::Actor;
use crate::auth::session::{self, CSRF_COOKIE, SESSION_COOKIE, SessionCtx};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::web::{ClientIp, ct_eq, get_cookie};

/// Request extension set by [`session_middleware`].
#[derive(Debug, Clone)]
pub struct CurrentSession(pub Option<Arc<SessionCtx>>);

/// True for requests that must carry a CSRF token.
pub fn needs_csrf(method: &Method, path: &str) -> bool {
    let safe = matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
    !safe && path.starts_with("/api/") && !path.starts_with("/api/webhooks/") && !path.starts_with("/api/public/")
}

/// Resolves the session once per request, stores [`CurrentSession`] and [`ClientIp`] as extensions and
/// enforces CSRF on unsafe `/api/**` requests.
pub async fn session_middleware(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    let ip = ClientIp::from_parts(req.headers(), peer, state.cfg.trust_proxy);
    req.extensions_mut().insert(ip);

    let path = req.uri().path().to_string();
    if !(path.starts_with("/api/") || path.starts_with("/mock/")) {
        return next.run(req).await;
    }
    let session = match get_cookie(req.headers(), SESSION_COOKIE) {
        Some(token) => match session::resolve(&state, &token).await {
            Ok(s) => s,
            Err(e) => return e.into_response(),
        },
        None => None,
    };
    if needs_csrf(req.method(), &path) {
        let sent = req.headers().get("x-csrf-token").and_then(|v| v.to_str().ok()).unwrap_or("");
        let expected = match &session {
            Some(s) => Some(s.csrf_token.clone()),
            None => get_cookie(req.headers(), CSRF_COOKIE),
        };
        let ok = matches!(&expected, Some(e) if !e.is_empty() && ct_eq(e, sent));
        if !ok {
            return AppError::forbidden_msg("Missing or invalid CSRF token. Reload the page and try again.")
                .into_response();
        }
    }
    req.extensions_mut().insert(CurrentSession(session.map(Arc::new)));
    next.run(req).await
}

/// The session for this request (from the middleware extension, or resolved from the cookie).
pub async fn current_session(parts: &Parts, state: &AppState) -> AppResult<Option<Arc<SessionCtx>>> {
    if let Some(CurrentSession(s)) = parts.extensions.get::<CurrentSession>() {
        return Ok(s.clone());
    }
    match get_cookie(&parts.headers, SESSION_COOKIE) {
        Some(token) => Ok(session::resolve(state, &token).await?.map(Arc::new)),
        None => Ok(None),
    }
}

/// The session if there is one (no TOTP requirement, never rejects) — for `/api/me` and logout.
#[derive(Debug, Clone)]
pub struct MaybeSession(pub Option<Arc<SessionCtx>>);

impl FromRequestParts<AppState> for MaybeSession {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        Ok(MaybeSession(current_session(parts, state).await?))
    }
}

/// Any valid session; TOTP not required.
#[derive(Debug, Clone)]
pub struct AuthSession(pub Arc<SessionCtx>);

impl FromRequestParts<AppState> for AuthSession {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        current_session(parts, state).await?.map(AuthSession).ok_or_else(AppError::unauthorized)
    }
}

impl FromRequestParts<AppState> for Actor {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let s = current_session(parts, state).await?.ok_or_else(AppError::unauthorized)?;
        if !s.actor.mfa_passed {
            return Err(AppError::mfa_required());
        }
        Ok(s.actor.clone())
    }
}

/// `Some` only for a fully authenticated actor.
#[derive(Debug, Clone)]
pub struct OptionalActor(pub Option<Actor>);

impl FromRequestParts<AppState> for OptionalActor {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let s = current_session(parts, state).await?;
        Ok(OptionalActor(s.filter(|s| s.actor.mfa_passed).map(|s| s.actor.clone())))
    }
}

/// A staff member who passed TOTP.
#[derive(Debug, Clone)]
pub struct StaffActor(pub Actor);

impl FromRequestParts<AppState> for StaffActor {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let s = current_session(parts, state).await?.ok_or_else(AppError::unauthorized)?;
        if !s.actor.is_staff() {
            return Err(AppError::forbidden());
        }
        if !s.actor.mfa_passed {
            return Err(AppError::mfa_required());
        }
        Ok(StaffActor(s.actor.clone()))
    }
}

impl std::ops::Deref for StaffActor {
    type Target = Actor;
    fn deref(&self) -> &Actor {
        &self.0
    }
}
