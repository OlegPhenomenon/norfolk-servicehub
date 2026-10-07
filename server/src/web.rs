//! HTTP helpers shared by every module's handlers: extractors whose rejections use the JSON error
//! envelope, the client IP, and cookie helpers.
//!
//! Use `crate::web::{Json, Path, Query}` instead of the axum originals so malformed input produces
//! `{"error": {"code": "validation", ...}}` rather than a plain-text body.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequest, FromRequestParts, Request};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::AppError;
use crate::state::AppState;

/// JSON body extractor / JSON response.
#[derive(Debug, Clone, Copy, Default)]
pub struct Json<T>(pub T);

impl<T, S> FromRequest<S> for Json<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(v)) => Ok(Json(v)),
            Err(rej) => Err(AppError::validation_msg(format!("Invalid request body: {}", rej.body_text()))),
        }
    }
}

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

/// Path parameters extractor; malformed parameters → `not_found`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Path<T>(pub T);

impl<T, S> FromRequestParts<S> for Path<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Path::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Path(v)) => Ok(Path(v)),
            Err(_) => Err(AppError::not_found()),
        }
    }
}

/// Query string extractor; malformed query → `validation`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Query<T>(pub T);

impl<T, S> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Query::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Query(v)) => Ok(Query(v)),
            Err(rej) => Err(AppError::validation_msg(format!("Invalid query string: {}", rej.body_text()))),
        }
    }
}

/// The client's IP address: first hop of `X-Forwarded-For` when `TRUST_PROXY=true`, else the socket peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientIp(pub String);

impl ClientIp {
    pub fn from_parts(headers: &HeaderMap, peer: Option<SocketAddr>, trust_proxy: bool) -> ClientIp {
        if trust_proxy
            && let Some(first) = headers
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.split(',').next())
                .map(str::trim)
                .filter(|v| !v.is_empty())
        {
            return ClientIp(first.to_string());
        }
        ClientIp(peer.map(|p| p.ip().to_string()).unwrap_or_else(|| "unknown".into()))
    }
}

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        if let Some(ip) = parts.extensions.get::<ClientIp>() {
            return Ok(ip.clone());
        }
        let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
        Ok(ClientIp::from_parts(&parts.headers, peer, state.cfg.trust_proxy))
    }
}

/// Value of cookie `name` from the request headers.
pub fn get_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| {
            let (k, v) = pair.trim().split_once('=')?;
            (k == name).then(|| v.to_string())
        })
        .next()
}

/// Builds a `Set-Cookie` header value. `max_age_secs = Some(0)` deletes the cookie.
pub fn cookie(name: &str, value: &str, max_age_secs: Option<i64>, http_only: bool, secure: bool) -> String {
    let mut c = format!("{name}={value}; Path=/; SameSite=Lax");
    if let Some(age) = max_age_secs {
        c.push_str(&format!("; Max-Age={age}"));
    }
    if http_only {
        c.push_str("; HttpOnly");
    }
    if secure {
        c.push_str("; Secure");
    }
    c
}

/// Constant-time string comparison (CSRF tokens, keys).
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_and_ip() {
        let mut h = HeaderMap::new();
        h.insert("cookie", "a=1; nsh_session=abc; b=2".parse().unwrap());
        assert_eq!(get_cookie(&h, "nsh_session").as_deref(), Some("abc"));
        assert_eq!(get_cookie(&h, "zzz"), None);
        h.insert("x-forwarded-for", "203.0.113.5, 10.0.0.1".parse().unwrap());
        assert_eq!(ClientIp::from_parts(&h, None, true).0, "203.0.113.5");
        assert_eq!(ClientIp::from_parts(&h, None, false).0, "unknown");
        assert!(ct_eq("abc", "abc") && !ct_eq("abc", "abd") && !ct_eq("abc", "ab"));
    }
}
