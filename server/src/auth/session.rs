//! Opaque session cookies. The cookie `nsh_session` holds 32 random bytes (hex); the database stores
//! only `sha256(token)` hex. Sessions slide: each request (at most once a minute) moves `last_seen_at`
//! and pushes `expires_at` to 12 h after the last activity.

use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

use crate::auth::Actor;
use crate::error::AppResult;
use crate::state::AppState;
use crate::time;

pub const SESSION_COOKIE: &str = "nsh_session";
/// Double-submit CSRF cookie used before a session exists (login, register, demo login).
pub const CSRF_COOKIE: &str = "nsh_csrf";
pub const SESSION_HOURS: i64 = 12;

/// A resolved, valid session.
#[derive(Debug, Clone)]
pub struct SessionCtx {
    pub token_hash: String,
    pub csrf_token: String,
    /// Actor as loaded for this session (`mfa_passed` reflects the session).
    pub actor: Actor,
    pub totp_enabled: bool,
}

/// 32 random bytes as hex.
pub fn random_token() -> String {
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Creates a session; returns `(cookie token, csrf token)`. Staff start with `mfa_passed = 0`.
pub async fn create(
    conn: &mut SqliteConnection,
    user_id: i64,
    mfa_passed: bool,
    now: DateTime<Utc>,
) -> AppResult<(String, String)> {
    let token = random_token();
    let csrf = random_token();
    let now_s = time::fmt(now);
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, csrf_token, mfa_passed, created_at, last_seen_at, expires_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(hash_token(&token))
    .bind(user_id)
    .bind(&csrf)
    .bind(i64::from(mfa_passed))
    .bind(&now_s)
    .bind(&now_s)
    .bind(time::fmt(now + Duration::hours(SESSION_HOURS)))
    .execute(&mut *conn)
    .await?;
    Ok((token, csrf))
}

/// Deletes a session by token hash.
pub async fn delete(conn: &mut SqliteConnection, token_hash: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?").bind(token_hash).execute(&mut *conn).await?;
    Ok(())
}

/// Marks the session as having passed TOTP.
pub async fn mark_mfa_passed(conn: &mut SqliteConnection, token_hash: &str) -> AppResult<()> {
    sqlx::query("UPDATE sessions SET mfa_passed = 1 WHERE token_hash = ?").bind(token_hash).execute(&mut *conn).await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    user_id: i64,
    csrf_token: String,
    mfa_passed: i64,
    last_seen_at: String,
    expires_at: String,
    totp_enabled: i64,
}

/// Resolves the session for a cookie token. Expired sessions are deleted and yield `None`.
pub async fn resolve(state: &AppState, token: &str) -> AppResult<Option<SessionCtx>> {
    let token_hash = hash_token(token);
    let mut conn = state.db.acquire().await?;
    let row: Option<SessionRow> = sqlx::query_as(
        "SELECT s.user_id, s.csrf_token, s.mfa_passed, s.last_seen_at, s.expires_at, u.totp_enabled \
         FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token_hash = ? AND u.is_active = 1",
    )
    .bind(&token_hash)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let now = state.now();
    if time::parse(&row.expires_at)? <= now {
        delete(&mut conn, &token_hash).await?;
        return Ok(None);
    }
    if now - time::parse(&row.last_seen_at)? > Duration::seconds(60) {
        sqlx::query("UPDATE sessions SET last_seen_at = ?, expires_at = ? WHERE token_hash = ?")
            .bind(time::fmt(now))
            .bind(time::fmt(now + Duration::hours(SESSION_HOURS)))
            .bind(&token_hash)
            .execute(&mut *conn)
            .await?;
    }
    let actor = match Actor::load(&mut conn, row.user_id, row.mfa_passed == 1).await {
        Ok(a) => a,
        Err(_) => return Ok(None),
    };
    Ok(Some(SessionCtx { token_hash, csrf_token: row.csrf_token, actor, totp_enabled: row.totp_enabled == 1 }))
}
