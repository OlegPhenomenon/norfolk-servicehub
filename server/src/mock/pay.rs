//! A separate, persistent test PSP. It sends signed webhooks over HTTP and never writes council payments.
use crate::{
    db,
    error::{AppError, AppResult},
    jobs,
    state::AppState,
    time,
    web::{Json, Path},
};
use axum::{
    Router,
    extract::State,
    http::HeaderMap,
    response::{Html, Redirect},
    routing::{get, post},
};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::{Row, SqliteConnection};
fn key(state: &AppState, headers: &HeaderMap) -> AppResult<()> {
    let value = headers.get("X-Mock-Key").and_then(|v| v.to_str().ok()).unwrap_or("");
    // Constant-time comparison without revealing a key prefix.
    let mut mac = Hmac::<Sha256>::new_from_slice(state.cfg.mock_api_key.as_bytes()).expect("HMAC key");
    mac.update(value.as_bytes());
    let mut expected = Hmac::<Sha256>::new_from_slice(state.cfg.mock_api_key.as_bytes()).expect("HMAC key");
    expected.update(state.cfg.mock_api_key.as_bytes());
    mac.verify_slice(&expected.finalize().into_bytes()).map_err(|_| AppError::forbidden())
}
fn token(prefix: &str) -> String {
    format!("{prefix}_{}", hex::encode(rand::random::<[u8; 16]>()))
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}
pub fn signature(secret: &str, t: i64, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC key");
    mac.update(format!("{t}.{body}").as_bytes());
    format!("t={t},v1={}", hex::encode(mac.finalize().into_bytes()))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/mock/pay/api/sessions", post(session))
        .route("/mock/pay/api/refunds", post(refund))
        .route("/mock/pay/checkout/{session}", get(checkout))
        .route("/mock/pay/checkout/{session}/{action}", post(pay))
}
#[derive(Deserialize)]
struct Session {
    amount_cents: i64,
    currency: String,
    reference: String,
    return_url: String,
    #[serde(default)]
    metadata: Value,
}
async fn session(State(state): State<AppState>, headers: HeaderMap, Json(b): Json<Session>) -> AppResult<Json<Value>> {
    key(&state, &headers)?;
    if b.amount_cents <= 0 || b.currency != "AUD" {
        return Err(AppError::field("amount_cents", "DemoPay accepts positive AUD amounts only."));
    }
    if !b.return_url.starts_with(&format!("{}/", state.cfg.public_base_url.trim_end_matches('/'))) {
        return Err(AppError::field("return_url", "Return URL must belong to this ServiceHub."));
    }
    let id = token("sess");
    let mut tx = db::write_tx(&state.db).await?;
    sqlx::query("INSERT INTO mock_pay_sessions(session_id,amount_cents,currency,reference,return_url,metadata_json,status,created_at) VALUES(?,?,?,?,?,?,'open',?)").bind(&id).bind(b.amount_cents).bind(b.currency).bind(b.reference).bind(b.return_url).bind(b.metadata.to_string()).bind(time::fmt(state.now())).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"session_id":id,"checkout_url":format!("{}/mock/pay/checkout/{id}",state.cfg.public_base_url)})))
}
async fn checkout(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Html<String>> {
    let r = sqlx::query("SELECT * FROM mock_pay_sessions WHERE session_id=?").bind(&id).fetch_one(&state.db).await?;
    let mut buttons = String::new();
    if r.get::<String, _>("status") == "open" {
        for (action, label) in [
            ("success", "Pay with test card"),
            ("decline", "Decline"),
            ("duplicate", "Pay — and deliver the webhook twice"),
            ("delayed", "Pay — webhook delayed 20 s"),
        ] {
            buttons.push_str(&format!("<form method=\"post\" action=\"/mock/pay/checkout/{}/{action}\"><button type=\"submit\">{label}</button></form>",escape(&id)));
        }
    } else {
        buttons = format!(
            "<p>This checkout is {}.</p><a href=\"{}\">Return to ServiceHub</a>",
            escape(&r.get::<String, _>("status")),
            escape(&r.get::<String, _>("return_url"))
        );
    }
    Ok(Html(format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>DemoPay — TEST MODE</title><style>body{{font:18px system-ui;max-width:36rem;margin:3rem auto;padding:1rem;line-height:1.5}}button{{font:inherit;min-height:44px;margin:.4rem 0;padding:.5rem 1rem;width:100%}}:focus-visible{{outline:3px solid #b07000}}</style></head><body><h1>DemoPay — TEST MODE</h1><p>No real money. Use a test button; no card details are collected.</p><p>Amount: <strong>{}</strong> AUD</p><p>Reference: {}</p>{buttons}</body></html>",
        crate::finance::ledger::money(r.get("amount_cents")),
        escape(&r.get::<String, _>("reference"))
    )))
}
async fn queue(tx: &mut SqliteConnection, state: &AppState, event: Value, delay: i64) -> AppResult<()> {
    let id = event["event_id"].as_str().ok_or_else(|| AppError::internal("Missing mock event id"))?;
    sqlx::query("INSERT INTO mock_pay_webhook_attempts(event_id,payload_json,created_at) VALUES(?,?,?)")
        .bind(id)
        .bind(event.to_string())
        .bind(time::fmt(state.now()))
        .execute(&mut *tx)
        .await?;
    jobs::enqueue(
        tx,
        "finance.mock_webhook",
        json!({"event_id":id}),
        Some(format!("mock-webhook:{id}")),
        state.now() + chrono::Duration::seconds(delay),
    )
    .await
}
async fn pay(State(state): State<AppState>, Path((id, action)): Path<(String, String)>) -> AppResult<Redirect> {
    if !matches!(action.as_str(), "success" | "decline" | "duplicate" | "delayed") {
        return Err(AppError::not_found());
    }
    let mut tx = db::write_tx(&state.db).await?;
    let r = sqlx::query("SELECT * FROM mock_pay_sessions WHERE session_id=?").bind(&id).fetch_one(&mut *tx).await?;
    let url: String = r.get("return_url");
    if r.get::<String, _>("status") == "open" {
        let payment = token("pay");
        let succeeded = action != "decline";
        sqlx::query("UPDATE mock_pay_sessions SET status=?,payment_id=? WHERE session_id=?")
            .bind(if succeeded { "paid" } else { "failed" })
            .bind(&payment)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        let event = json!({"event_id":token("evt"),"type":if succeeded{"payment.succeeded"}else{"payment.failed"},"payment_id":payment,"session_id":id,"amount_cents":r.get::<i64,_>("amount_cents")});
        queue(&mut tx, &state, event.clone(), if action == "delayed" { 20 } else { 1 }).await?;
        if action == "duplicate" {
            jobs::enqueue(
                &mut tx,
                "finance.mock_webhook",
                json!({"event_id":event["event_id"],"redeliver":true}),
                None,
                state.now() + chrono::Duration::seconds(2),
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(Redirect::to(&url))
}
#[derive(Deserialize)]
struct Refund {
    payment_id: String,
    amount_cents: i64,
    idempotency_key: String,
}
async fn refund(State(state): State<AppState>, headers: HeaderMap, Json(b): Json<Refund>) -> AppResult<Json<Value>> {
    key(&state, &headers)?;
    let mut tx = db::write_tx(&state.db).await?;
    if let Some((id, payment, cents, status)) = sqlx::query_as::<_, (String, String, i64, String)>(
        "SELECT refund_id,payment_id,amount_cents,status FROM mock_pay_refunds WHERE idempotency_key=?",
    )
    .bind(&b.idempotency_key)
    .fetch_optional(&mut *tx)
    .await?
    {
        if payment != b.payment_id || cents != b.amount_cents {
            return Err(AppError::idempotency_mismatch());
        }
        return Ok(Json(json!({"refund_id":id,"status":status})));
    }
    let paid: Option<i64> =
        sqlx::query_scalar("SELECT amount_cents FROM mock_pay_sessions WHERE payment_id=? AND status='paid'")
            .bind(&b.payment_id)
            .fetch_optional(&mut *tx)
            .await?;
    let reserved:i64=sqlx::query_scalar("SELECT COALESCE(SUM(amount_cents),0) FROM mock_pay_refunds WHERE payment_id=? AND status IN ('pending','succeeded')").bind(&b.payment_id).fetch_one(&mut *tx).await?;
    if b.amount_cents <= 0 || paid.is_none_or(|a| b.amount_cents > a - reserved) {
        return Err(AppError::field("amount_cents", "Refund exceeds confirmed provider money."));
    }
    let id = token("ref");
    sqlx::query("INSERT INTO mock_pay_refunds(refund_id,payment_id,amount_cents,idempotency_key,status,created_at) VALUES(?,?,?,?,'pending',?)").bind(&id).bind(&b.payment_id).bind(b.amount_cents).bind(&b.idempotency_key).bind(time::fmt(state.now())).execute(&mut *tx).await?;
    jobs::enqueue(
        &mut tx,
        "finance.mock_refund",
        json!({"refund_id":id}),
        Some(format!("mock-refund:{id}")),
        state.now() + chrono::Duration::seconds(3),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"refund_id":id,"status":"pending"})))
}
pub async fn handle_job(state: &AppState, kind: &str, payload: &Value) -> AppResult<()> {
    if kind == "finance.mock_refund" {
        let id = payload["refund_id"].as_str().ok_or_else(|| AppError::internal("Missing refund id"))?;
        let mut tx = db::write_tx(&state.db).await?;
        let r = sqlx::query("SELECT * FROM mock_pay_refunds WHERE refund_id=?").bind(id).fetch_one(&mut *tx).await?;
        if r.get::<String, _>("status") != "pending" {
            return Ok(());
        }
        let fail = r.get::<i64, _>("amount_cents") % 100 == 13;
        sqlx::query("UPDATE mock_pay_refunds SET status=? WHERE refund_id=?")
            .bind(if fail { "failed" } else { "succeeded" })
            .bind(id)
            .execute(&mut *tx)
            .await?;
        queue(&mut tx,state,json!({"event_id":token("evt"),"type":if fail{"refund.failed"}else{"refund.succeeded"},"refund_id":id,"payment_id":r.get::<String,_>("payment_id"),"amount_cents":r.get::<i64,_>("amount_cents")}),0).await?;
        tx.commit().await?;
        return Ok(());
    }
    let id = payload["event_id"].as_str().ok_or_else(|| AppError::internal("Missing event id"))?;
    let r =
        sqlx::query("SELECT * FROM mock_pay_webhook_attempts WHERE event_id=?").bind(id).fetch_one(&state.db).await?;
    if r.get::<Option<String>, _>("delivered_at").is_some() && !payload["redeliver"].as_bool().unwrap_or(false) {
        return Ok(());
    }
    let body: String = r.get("payload_json");
    let result = state
        .http
        .post(format!("{}/api/webhooks/demopay", state.cfg.internal_base_url))
        .header("DemoPay-Signature", signature(&state.cfg.webhook_secret, state.now().timestamp(), &body))
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await;
    let success = result.as_ref().is_ok_and(|r| r.status().is_success());
    let err = if success {
        None
    } else {
        Some(match result {
            Ok(r) => format!("HTTP {}", r.status()),
            Err(e) => e.to_string(),
        })
    };
    let mut tx = db::write_tx(&state.db).await?;
    sqlx::query("UPDATE mock_pay_webhook_attempts SET attempts=attempts+1,delivered_at=COALESCE(?,delivered_at),last_error=? WHERE event_id=?").bind(if success{Some(time::fmt(state.now()))}else{None}).bind(&err).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    if let Some(e) = err {
        return Err(AppError::internal(e));
    }
    Ok(())
}
