//! Application error type and the JSON error envelope.
//!
//! Every handler returns [`AppResult<T>`]. Errors render as
//! `{"error": {"code": "...", "message": "Human sentence", "fields": {"field": "message"}}}`
//! with the HTTP status implied by the code (see `docs/ARCHITECTURE.md` §3).
//!
//! Never leak the existence of a case the actor cannot see: use [`AppError::not_found`].

use std::collections::BTreeMap;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// Result alias used across the crate.
pub type AppResult<T> = Result<T, AppError>;

/// Machine-readable error codes (the `code` field of the envelope).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Unauthorized,
    MfaRequired,
    Forbidden,
    NotFound,
    Conflict,
    StaleRevision,
    IdempotencyMismatch,
    Validation,
    RateLimited,
    /// 410 — only used when a public demo has ended (`DEMO_ENDS_AT`).
    DemoEnded,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Unauthorized => "unauthorized",
            ErrorCode::MfaRequired => "mfa_required",
            ErrorCode::Forbidden => "forbidden",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Conflict => "conflict",
            ErrorCode::StaleRevision => "stale_revision",
            ErrorCode::IdempotencyMismatch => "idempotency_mismatch",
            ErrorCode::Validation => "validation",
            ErrorCode::RateLimited => "rate_limited",
            ErrorCode::DemoEnded => "demo_ended",
            ErrorCode::Internal => "internal",
        }
    }

    pub fn status(self) -> StatusCode {
        match self {
            ErrorCode::Unauthorized | ErrorCode::MfaRequired => StatusCode::UNAUTHORIZED,
            ErrorCode::Forbidden => StatusCode::FORBIDDEN,
            ErrorCode::NotFound => StatusCode::NOT_FOUND,
            ErrorCode::Conflict | ErrorCode::StaleRevision | ErrorCode::IdempotencyMismatch => StatusCode::CONFLICT,
            ErrorCode::Validation => StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::DemoEnded => StatusCode::GONE,
            ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// The single error type of the application.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct AppError {
    pub code: ErrorCode,
    /// Human-readable sentence shown to the user. For `internal` errors the client always
    /// receives a generic sentence; the detailed message is only logged.
    pub message: String,
    /// Per-field validation messages (`validation` errors).
    pub fields: BTreeMap<String, String>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        AppError { code, message: message.into(), fields: BTreeMap::new() }
    }

    /// 401 — no valid session.
    pub fn unauthorized() -> Self {
        Self::new(ErrorCode::Unauthorized, "Please sign in to continue.")
    }

    /// 401 — staff session has not passed the TOTP step yet.
    pub fn mfa_required() -> Self {
        Self::new(ErrorCode::MfaRequired, "Enter the code from your authenticator app to continue.")
    }

    /// 403 — authenticated but not allowed.
    pub fn forbidden() -> Self {
        Self::new(ErrorCode::Forbidden, "You do not have permission to do this.")
    }

    /// 403 with a specific explanation.
    pub fn forbidden_msg(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Forbidden, msg)
    }

    /// 404 — also used when the actor may not know the entity exists.
    pub fn not_found() -> Self {
        Self::new(ErrorCode::NotFound, "Not found.")
    }

    /// 409 — business conflict (double booking, duplicate, wrong state…).
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, msg)
    }

    /// 409 — `expected_revision` did not match.
    pub fn stale_revision() -> Self {
        Self::new(ErrorCode::StaleRevision, "Someone else changed this record. Reload to see the latest version.")
    }

    /// 409 — same `Idempotency-Key` reused with a different request body.
    pub fn idempotency_mismatch() -> Self {
        Self::new(ErrorCode::IdempotencyMismatch, "This request key was already used for a different request.")
    }

    /// 422 — field-level validation errors.
    pub fn validation<K: Into<String>, V: Into<String>>(fields: impl IntoIterator<Item = (K, V)>) -> Self {
        let fields: BTreeMap<String, String> = fields.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
        AppError { code: ErrorCode::Validation, message: "Please correct the highlighted fields.".into(), fields }
    }

    /// 422 for a single field.
    pub fn field(field: impl Into<String>, msg: impl Into<String>) -> Self {
        Self::validation([(field.into(), msg.into())])
    }

    /// 422 without field details.
    pub fn validation_msg(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Validation, msg)
    }

    /// 429.
    pub fn rate_limited() -> Self {
        Self::new(ErrorCode::RateLimited, "Too many attempts. Please wait a few minutes and try again.")
    }

    /// 500 — the message is logged, never shown to the client.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, msg)
    }

    pub fn status(&self) -> StatusCode {
        self.code.status()
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let message = if self.code == ErrorCode::Internal {
            tracing::error!(error = %self.message, "internal error");
            "Something went wrong on our side. Please try again.".to_string()
        } else {
            self.message
        };
        let mut body = json!({ "code": self.code.as_str(), "message": message });
        if !self.fields.is_empty() {
            body["fields"] = json!(self.fields);
        }
        (self.code.status(), Json(json!({ "error": body }))).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        match &e {
            sqlx::Error::RowNotFound => AppError::not_found(),
            sqlx::Error::Database(db) => {
                let msg = db.message();
                if msg.contains("occupancy_conflict") {
                    AppError::conflict("That time is no longer available.")
                } else if msg.contains("price_overlap") {
                    AppError::conflict("Price periods for one item must not overlap.")
                } else if db.is_unique_violation() || msg.contains("UNIQUE constraint failed") {
                    AppError::conflict("This record already exists.")
                } else {
                    AppError::internal(format!("database error: {e}"))
                }
            }
            _ => AppError::internal(format!("database error: {e}")),
        }
    }
}

impl From<sqlx::migrate::MigrateError> for AppError {
    fn from(e: sqlx::migrate::MigrateError) -> Self {
        AppError::internal(format!("migration error: {e}"))
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::internal(format!("json error: {e}"))
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::internal(format!("io error: {e}"))
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::internal(format!("{e:#}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_map_to_statuses() {
        assert_eq!(AppError::not_found().status(), StatusCode::NOT_FOUND);
        assert_eq!(AppError::stale_revision().status(), StatusCode::CONFLICT);
        assert_eq!(AppError::field("x", "bad").status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(AppError::mfa_required().status(), StatusCode::UNAUTHORIZED);
    }
}
