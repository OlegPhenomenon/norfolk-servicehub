//! Idempotent mock receivers. Stored state survives sender timeouts.
use crate::{
    db::write_tx,
    error::{AppError, AppResult},
    records::integrations::Accepted,
    state::AppState,
    time,
    web::{Json, Path},
};
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::Value;

pub fn routes() -> Router<AppState> {
    Router::new().route("/mock/records/{code}/api/records", post(receive))
}
async fn receive(
    State(state): State<AppState>,
    Path(code): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> AppResult<Response> {
    crate::mock::require_mock_key(&state, &headers)?;
    let operation = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty() && s.len() <= 200)
        .ok_or_else(|| AppError::field("Idempotency-Key", "An operation identifier is required."))?;
    let mut tx = write_tx(&state.db).await?;
    let switches: Option<(i64, i64)> =
        sqlx::query_as("SELECT outage,drop_responses FROM mock_system_state WHERE system_code=?")
            .bind(&code)
            .fetch_optional(&mut *tx)
            .await?;
    let (outage, drop) = switches.ok_or_else(AppError::not_found)?;
    if outage != 0 {
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({"error":{"code":"unavailable","message":"Mock records system is offline."}})),
        )
            .into_response());
    }
    let existing: Option<String> =
        sqlx::query_scalar("SELECT external_ref FROM mock_external_records WHERE system_code=? AND operation_id=?")
            .bind(&code)
            .bind(operation)
            .fetch_optional(&mut *tx)
            .await?;
    let (reference, new) = match existing {
        Some(r) => (r, false),
        None => {
            let seq: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id),4210)+1 FROM mock_external_records")
                .fetch_one(&mut *tx)
                .await?;
            let prefix = if code == "content_manager" { "CM" } else { "CA" };
            let reference = format!("{prefix}-{}-{seq:06}", time::to_local(state.now()).format("%Y"));
            sqlx::query("INSERT INTO mock_external_records(system_code,operation_id,external_ref,payload_json,received_at) VALUES(?,?,?,?,?)")
                .bind(&code).bind(operation).bind(&reference).bind(payload.to_string()).bind(time::fmt(state.now())).execute(&mut *tx).await?;
            (reference, true)
        }
    };
    tx.commit().await?;
    // Only the first acceptance loses its response. A retry returns the saved reference immediately.
    if drop != 0 && new {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
    Ok((if new { StatusCode::CREATED } else { StatusCode::OK }, Json(Accepted { external_ref: reference }))
        .into_response())
}
