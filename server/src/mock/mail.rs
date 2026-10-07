// OWNER: platform
//! DemoMail: the mock email/SMS gateway. `POST /mock/mail/send` (requires `X-Mock-Key`) accepts a
//! message and returns `{id}`; addresses ending in `@bounce.example` are permanently rejected (422).
//! Sent messages are visible through `GET /api/demo/mailbox` (the `notifications` table).

use axum::Router;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::web::Json;

pub fn routes() -> Router<AppState> {
    Router::new().route("/mock/mail/send", post(send))
}

#[derive(Debug, Deserialize)]
struct SendBody {
    channel: String,
    to: Option<String>,
    subject: String,
    body: String,
}

#[derive(Debug, Serialize)]
struct SendResponse {
    id: String,
}

async fn send(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(msg): Json<SendBody>,
) -> AppResult<Json<SendResponse>> {
    super::require_mock_key(&st, &headers)?;
    let to = msg.to.as_deref().map(str::trim).unwrap_or("");
    if !matches!(msg.channel.as_str(), "email" | "sms") || to.is_empty() {
        return Err(AppError::validation_msg("DemoMail needs a channel (email or sms) and a recipient."));
    }
    if to.to_ascii_lowercase().ends_with("@bounce.example") {
        return Err(AppError::validation_msg(format!("Mailbox {to} does not exist (permanent failure).")));
    }
    let mut b = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut b);
    tracing::info!(channel = %msg.channel, subject_len = msg.subject.len(), body_len = msg.body.len(), "DemoMail accepted a message");
    Ok(Json(SendResponse { id: format!("dm_{}", hex::encode(b)) }))
}
