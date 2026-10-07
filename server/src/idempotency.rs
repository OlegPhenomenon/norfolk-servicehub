//! Idempotent POSTs (`Idempotency-Key` header): same key + same body returns the stored response;
//! same key + different body → `idempotency_mismatch` (409).
//!
//! Typical handler:
//! ```ignore
//! async fn submit(State(st): State<AppState>, actor: Actor, IdempotencyKey(key): IdempotencyKey,
//!                 body: axum::body::Bytes) -> AppResult<(StatusCode, Json<Value>)> {
//!     let mut tx = db::write_tx(&st.db).await?;
//!     let hash = idempotency::request_hash(&body);
//!     let (status, json) = idempotency::idempotent(&mut tx, actor.user_id, "case.submit", key.as_deref(), &hash,
//!         |tx| Box::pin(async move { /* do the work with tx */ Ok((StatusCode::CREATED, json!({"id": 1}))) })).await?;
//!     tx.commit().await?;
//!     Ok((status, Json(json)))
//! }
//! ```

use std::pin::Pin;

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

use crate::error::{AppError, AppResult};
use crate::time;

/// Extractor for the optional `Idempotency-Key` header (max 200 chars).
#[derive(Debug, Clone)]
pub struct IdempotencyKey(pub Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for IdempotencyKey {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        match parts.headers.get("idempotency-key") {
            None => Ok(IdempotencyKey(None)),
            Some(v) => {
                let s = v.to_str().map_err(|_| AppError::validation_msg("Invalid Idempotency-Key header."))?.trim();
                if s.is_empty() || s.len() > 200 {
                    return Err(AppError::validation_msg("Idempotency-Key must be 1–200 characters."));
                }
                Ok(IdempotencyKey(Some(s.to_string())))
            }
        }
    }
}

/// sha256 hex of the raw request body.
pub fn request_hash(body: &[u8]) -> String {
    hex::encode(Sha256::digest(body))
}

/// sha256 hex of a JSON value's canonical serialisation (keys are sorted by `serde_json`'s map).
pub fn json_hash(v: &Value) -> String {
    request_hash(v.to_string().as_bytes())
}

/// A stored response.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    pub status: StatusCode,
    pub body: Value,
}

/// Looks up a previous response. `Err(idempotency_mismatch)` if the key was used with another body.
pub async fn lookup(
    conn: &mut SqliteConnection,
    actor_user_id: i64,
    scope: &str,
    key: &str,
    request_hash: &str,
) -> AppResult<Option<Stored>> {
    let row: Option<(String, i64, String)> = sqlx::query_as(
        "SELECT request_hash, status_code, response_json FROM idempotency_keys WHERE actor_user_id = ? AND scope = ? AND key = ?",
    )
    .bind(actor_user_id)
    .bind(scope)
    .bind(key)
    .fetch_optional(&mut *conn)
    .await?;
    match row {
        None => Ok(None),
        Some((hash, _, _)) if hash != request_hash => Err(AppError::idempotency_mismatch()),
        Some((_, status, body)) => Ok(Some(Stored {
            status: StatusCode::from_u16(status as u16).unwrap_or(StatusCode::OK),
            body: serde_json::from_str(&body)?,
        })),
    }
}

/// Stores a response for later replays (same transaction as the work it describes).
pub async fn store(
    conn: &mut SqliteConnection,
    actor_user_id: i64,
    scope: &str,
    key: &str,
    request_hash: &str,
    status: StatusCode,
    body: &Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO idempotency_keys (actor_user_id, scope, key, request_hash, status_code, response_json, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(actor_user_id)
    .bind(scope)
    .bind(key)
    .bind(request_hash)
    .bind(i64::from(status.as_u16()))
    .bind(body.to_string())
    .bind(time::now_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Boxed future returned by the work closure of [`idempotent`].
pub type WorkFuture<'c> = Pin<Box<dyn Future<Output = AppResult<(StatusCode, Value)>> + Send + 'c>>;

/// Runs `work` once per (`actor_user_id`, `scope`, `key`) inside the caller's transaction.
/// Without a key, `work` simply runs. Errors from `work` are not stored (the caller's tx rolls back).
/// `actor_user_id` is 0 for unauthenticated callers.
pub async fn idempotent<F>(
    conn: &mut SqliteConnection,
    actor_user_id: i64,
    scope: &str,
    key: Option<&str>,
    request_hash: &str,
    work: F,
) -> AppResult<(StatusCode, Value)>
where
    F: for<'c> FnOnce(&'c mut SqliteConnection) -> WorkFuture<'c>,
{
    let Some(key) = key else {
        return work(conn).await;
    };
    if let Some(prev) = lookup(conn, actor_user_id, scope, key, request_hash).await? {
        return Ok((prev.status, prev.body));
    }
    let (status, body) = work(&mut *conn).await?;
    store(conn, actor_user_id, scope, key, request_hash, status, &body).await?;
    Ok((status, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::write_tx;
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn same_key_same_body_replays_and_different_body_conflicts() {
        let (state, _dir) = crate::state::test_support::test_state().await;
        let calls = Arc::new(AtomicUsize::new(0));
        let body_a = br#"{"answers":{"a":1}}"#;
        let body_b = br#"{"answers":{"a":2}}"#;

        let run = |hash: String, calls: Arc<AtomicUsize>| {
            let db = state.db.clone();
            async move {
                let mut tx = write_tx(&db).await?;
                let res = idempotent(&mut tx, 7, "case.submit", Some("key-1"), &hash, move |tx| {
                    Box::pin(async move {
                        let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
                        sqlx::query("INSERT INTO settings (key, value_json, updated_at) VALUES (?, '1', 'x')")
                            .bind(format!("work.{n}"))
                            .execute(&mut *tx)
                            .await?;
                        Ok((StatusCode::CREATED, json!({ "case_id": 42, "call": n })))
                    })
                })
                .await;
                if res.is_ok() {
                    tx.commit().await?;
                }
                res
            }
        };

        let first = run(request_hash(body_a), calls.clone()).await.unwrap();
        let second = run(request_hash(body_a), calls.clone()).await.unwrap();
        assert_eq!(first, second);
        assert_eq!(first.0, StatusCode::CREATED);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "work runs once");

        let err = run(request_hash(body_b), calls.clone()).await.unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::IdempotencyMismatch);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Different actor, same key: independent.
        let mut tx = write_tx(&state.db).await.unwrap();
        let other = idempotent(&mut tx, 8, "case.submit", Some("key-1"), &request_hash(body_b), |_tx| {
            Box::pin(async { Ok((StatusCode::OK, json!({"other": true}))) })
        })
        .await
        .unwrap();
        assert_eq!(other.1, json!({"other": true}));
    }
}
