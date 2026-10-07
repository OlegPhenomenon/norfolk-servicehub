use super::{deposits, ledger, payments};
use crate::{
    auth::Actor,
    db,
    error::{AppError, AppResult},
    state::AppState,
    time,
};
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;
use sqlx::{Row, SqliteConnection};
#[derive(Debug, Serialize, Deserialize)]
pub struct Event {
    pub event_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub payment_id: Option<String>,
    #[serde(default)]
    pub refund_id: Option<String>,
    #[serde(default)]
    pub amount_cents: Option<i64>,
}
pub fn verify(secret: &str, header: &str, body: &[u8], now: i64) -> bool {
    let mut timestamp = None;
    let mut signature = None;
    for part in header.split(',') {
        if let Some(t) = part.strip_prefix("t=") {
            if timestamp.is_some() {
                return false;
            }
            timestamp = t.parse::<i64>().ok();
        }
        if let Some(v) = part.strip_prefix("v1=") {
            if signature.is_some() {
                return false;
            }
            signature = hex::decode(v).ok();
        }
    }
    let (Some(t), Some(sig)) = (timestamp, signature) else {
        return false;
    };
    if (i128::from(now) - i128::from(t)).abs() > 300 {
        return false;
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC key");
    mac.update(format!("{t}.").as_bytes());
    mac.update(body);
    mac.verify_slice(&sig).is_ok()
}
/// Returns a case to advance only for newly confirmed money; transaction rollback preserves retryability.
pub async fn process(tx: &mut SqliteConnection, e: &Event, body: &str) -> AppResult<Option<(i64, Option<i64>)>> {
    let duplicate: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM provider_events WHERE event_id=? AND signature_valid=1)")
            .bind(&e.event_id)
            .fetch_one(&mut *tx)
            .await?;
    if duplicate {
        return Ok(None);
    }
    if e.event_id.trim().is_empty() {
        return Err(AppError::validation_msg("Event ID is required."));
    }
    // Invalid-signature attempts use their body hash, so they cannot poison a genuine event ID.
    sqlx::query(
        "INSERT INTO provider_events(event_id,type,payload_json,signature_valid,received_at) VALUES(?,?,?,1,?)",
    )
    .bind(&e.event_id)
    .bind(&e.kind)
    .bind(body)
    .bind(time::now_str())
    .execute(&mut *tx)
    .await?;
    let mut advance = None;
    match e.kind.as_str() {
        "payment.succeeded" | "payment.failed" => {
            let session = e.session_id.as_deref().ok_or_else(|| AppError::validation_msg("Session ID required."))?;
            let r = sqlx::query("SELECT * FROM checkout_sessions WHERE provider_session_id=?")
                .bind(session)
                .fetch_one(&mut *tx)
                .await?;
            let case: i64 = r.get("case_id");
            if e.kind == "payment.failed" {
                let updated=sqlx::query("UPDATE checkout_sessions SET status='failed',completed_at=? WHERE provider_session_id=? AND status='open'").bind(time::now_str()).bind(session).execute(&mut *tx).await?;
                if updated.rows_affected() == 0 {
                    sqlx::query(
                        "UPDATE provider_events SET processed_at=?,result='duplicate_failure' WHERE event_id=?",
                    )
                    .bind(time::now_str())
                    .bind(&e.event_id)
                    .execute(&mut *tx)
                    .await?;
                    return Ok(None);
                }
                ledger::event(
                    tx,
                    &Actor::system(),
                    case,
                    "finance.payment_failed",
                    "DemoPay declined the test payment; charges remain unpaid",
                    json!({"session_id":session}),
                )
                .await?;
                ledger::tell(
                    tx,
                    case,
                    "Payment declined",
                    "DemoPay declined the test payment. You can try again from your request.",
                )
                .await?;
            } else {
                let payment =
                    e.payment_id.as_deref().ok_or_else(|| AppError::validation_msg("Payment ID required."))?;
                let cents = e.amount_cents.ok_or_else(|| AppError::validation_msg("Amount required."))?;
                // Second guard is independent of event IDs. Allow a short or excess receipt and allocate safely.
                let (id, new) = payments::receive(
                    tx,
                    &Actor::system(),
                    "provider",
                    payment,
                    cents,
                    Some(case),
                    Some(r.get("invoice_id")),
                    None,
                    Some(session),
                )
                .await?;
                // A genuine late receipt from an expired checkout is still money received.
                // External payment IDs dedupe it; excess over the now-settled invoice becomes credit.
                sqlx::query("UPDATE checkout_sessions SET status='paid',completed_at=? WHERE provider_session_id=?")
                    .bind(time::now_str())
                    .bind(session)
                    .execute(&mut *tx)
                    .await?;
                if payments::invoice_due(tx, case, r.get("invoice_id")).await? == 0 {
                    sqlx::query("UPDATE checkout_sessions SET status='expired',completed_at=? WHERE invoice_id=? AND status='open'").bind(time::now_str()).bind(r.get::<i64,_>("invoice_id")).execute(&mut *tx).await?;
                }
                if new {
                    advance = Some((case, Some(id)));
                }
            }
        }
        "refund.succeeded" | "refund.failed" => {
            let provider = e.refund_id.as_deref().ok_or_else(|| AppError::validation_msg("Refund ID required."))?;
            let r=sqlx::query("SELECT r.*,p.external_id FROM refunds r JOIN payments p ON p.id=r.payment_id WHERE r.provider_refund_id=?").bind(provider).fetch_one(&mut *tx).await?;
            if e.amount_cents != Some(r.get("amount_cents"))
                || e.payment_id.as_deref() != Some(r.get::<String, _>("external_id").as_str())
            {
                return Err(AppError::validation_msg("Refund confirmation does not match the reserved refund."));
            }
            let id: i64 = r.get("id");
            let case: i64 = r.get("case_id");
            if e.kind == "refund.succeeded" {
                if let Some(c) = deposits::complete(tx, &Actor::system(), id, None).await? {
                    advance = Some((c, None));
                }
            } else if r.get::<String, _>("status") == "processing" {
                sqlx::query("UPDATE refunds SET status='failed',failure_reason='DemoPay test failure (amount ends in 13 cents)' WHERE id=?").bind(id).execute(&mut *tx).await?;
                ledger::event(
                    tx,
                    &Actor::system(),
                    case,
                    "finance.refund_failed",
                    "DemoPay refund failed; finance will arrange the refund",
                    json!({"refund_id":id}),
                )
                .await?;
                ledger::tell(
                    tx,
                    case,
                    "Refund needs attention",
                    "Your refund has not completed. Finance has been notified to arrange it.",
                )
                .await?;
                ledger::finance_notice(tx, case, "DemoPay refund failed — action required").await?;
            }
        }
        _ => return Err(AppError::validation_msg("Unknown DemoPay event type.")),
    }
    sqlx::query("UPDATE provider_events SET processed_at=?,result='processed' WHERE event_id=?")
        .bind(time::now_str())
        .bind(&e.event_id)
        .execute(&mut *tx)
        .await?;
    Ok(advance)
}
pub async fn handler(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> AppResult<Response> {
    let header = headers.get("DemoPay-Signature").and_then(|v| v.to_str().ok()).unwrap_or_default();
    let _permit = acquire(&WEBHOOKS)?;
    if body.len() > MAX_WEBHOOK_BYTES {
        return Ok(StatusCode::PAYLOAD_TOO_LARGE.into_response());
    }
    let text = String::from_utf8_lossy(&body);
    if !verify(&state.cfg.webhook_secret, header, &body, state.now().timestamp()) {
        return Ok((
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"error":{"code":"validation","message":"Invalid DemoPay signature."}})),
        )
            .into_response());
    }
    let e: Event =
        serde_json::from_slice(&body).map_err(|_| AppError::validation_msg("Invalid payment event JSON."))?;
    let mut tx = db::write_tx(&state.db).await?;
    if let Some((case, payment)) = process(&mut tx, &e, &text).await? {
        if let Some(p) = payment {
            crate::records::api::on_payment_confirmed(&mut tx, p).await?;
        }
        crate::cases::workflow::try_auto_advance(&mut tx, &state, case).await?;
    }
    tx.commit().await?;
    Ok(StatusCode::OK.into_response())
}

pub const MAX_WEBHOOK_BYTES: usize = 16 * 1024;
static WEBHOOKS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
fn acquire(semaphore: &tokio::sync::Semaphore) -> AppResult<tokio::sync::SemaphorePermit<'_>> {
    semaphore.try_acquire().map_err(|_| AppError::rate_limited())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn webhook_concurrency_is_bounded_before_writer() {
        let semaphore = tokio::sync::Semaphore::new(2);
        let permits = semaphore.acquire_many(2).await.unwrap();
        assert_eq!(acquire(&semaphore).unwrap_err().code, crate::error::ErrorCode::RateLimited);
        drop(permits);
    }
}
