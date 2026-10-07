use super::ledger;
use crate::{
    auth::Actor,
    error::{AppError, AppResult},
    jobs,
    state::AppState,
    time,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
#[derive(Debug, Serialize, Deserialize)]
pub struct RetainItem {
    pub label: String,
    pub cents: i64,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Decision {
    pub invoice_line_id: i64,
    pub refund_cents: i64,
    pub retain_items: Vec<RetainItem>,
    pub reason: String,
}

pub async fn ready(tx: &mut SqliteConnection, case: i64, now: &str) -> AppResult<bool> {
    let unused_cancelled: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bookings b JOIN booking_cancellations x ON x.booking_id=b.id WHERE b.case_id=? AND b.status='cancelled' AND x.unused=1)").bind(case).fetch_one(&mut *tx).await?;
    if unused_cancelled {
        return Ok(true);
    }
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bookings WHERE case_id=? AND end_at<=? AND status IN ('confirmed','completed','cancelled')) AND EXISTS(SELECT 1 FROM tasks WHERE case_id=? AND kind='venue_inspection' AND status='done') AND NOT EXISTS(SELECT 1 FROM tasks WHERE case_id=? AND kind='venue_inspection' AND status NOT IN ('done','cancelled'))").bind(case).bind(now).bind(case).bind(case).fetch_one(&mut *tx).await?)
}
/// Unique decision per deposit line plus BEGIN IMMEDIATE reserves the refund atomically.
pub async fn decide(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: i64,
    d: &Decision,
) -> AppResult<i64> {
    if d.reason.trim().is_empty() {
        return Err(AppError::field("reason", "Explain the bond decision for the applicant."));
    }
    if d.refund_cents < 0 {
        return Err(AppError::field("refund_cents", "Refund cannot be negative."));
    }
    if d.retain_items.iter().any(|i| i.cents <= 0 || i.label.trim().is_empty()) {
        return Err(AppError::field("retain_items", "Each retained item needs a reason and a positive amount."));
    }
    let retained = d
        .retain_items
        .iter()
        .try_fold(0i64, |a, i| a.checked_add(i.cents))
        .ok_or_else(|| AppError::field("retain_items", "Amount is too large."))?;
    let r=sqlx::query("SELECT l.* FROM finance_line_balances l JOIN invoices i ON i.id=l.invoice_id WHERE l.id=? AND i.case_id=? AND i.kind='invoice' AND i.status='issued' AND l.kind='deposit'").bind(d.invoice_line_id).bind(case).fetch_one(&mut *tx).await?;
    let paid: i64 = r.get("paid_cents");
    if paid == 0 || paid != r.get::<i64, _>("amount_cents") - r.get::<i64, _>("credited_cents") {
        return Err(AppError::conflict("The refundable bond must be fully paid before a decision."));
    }
    if d.refund_cents.checked_add(retained) != Some(paid) {
        return Err(AppError::field("refund_cents", "Refund plus itemised retention must equal the bond paid."));
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM deposit_decisions WHERE invoice_line_id=?)")
        .bind(d.invoice_line_id)
        .fetch_one(&mut *tx)
        .await?;
    if exists {
        return Err(AppError::conflict("This bond already has a decision and its refund is reserved."));
    }
    if !ready(tx, case, &time::fmt(state.now())).await? {
        return Err(AppError::conflict("Wait until the event has finished and the hall inspection is complete."));
    }
    let id:i64=sqlx::query_scalar("INSERT INTO deposit_decisions(case_id,invoice_line_id,refund_cents,retain_cents,reason,calc_json,decided_by,decided_at) VALUES(?,?,?,?,?,?,?,?) RETURNING id").bind(case).bind(d.invoice_line_id).bind(d.refund_cents).bind(retained).bind(&d.reason).bind(serde_json::to_string(&d.retain_items)?).bind(actor.db_id()).bind(time::fmt(state.now())).fetch_one(&mut *tx).await?;
    let sources:Vec<(i64,String,i64,i64)>=sqlx::query_as("SELECT p.id,p.source,SUM(a.amount_cents),p.amount_cents-COALESCE((SELECT SUM(amount_cents) FROM refunds WHERE payment_id=p.id),0) FROM payment_allocations a JOIN payments p ON p.id=a.payment_id WHERE a.invoice_line_id=? AND a.reversed_at IS NULL AND p.status='confirmed' GROUP BY p.id ORDER BY p.id").bind(d.invoice_line_id).fetch_all(&mut *tx).await?;
    let mut remaining = d.refund_cents;
    for (payment, source, funded, available) in sources {
        let cents = remaining.min(funded).min(available);
        if cents > 0 {
            let refund:i64=sqlx::query_scalar("INSERT INTO refunds(case_id,payment_id,deposit_decision_id,amount_cents,method,status,reason,requested_by,created_at) VALUES(?,?,?,?,?,'processing',?,?,?) RETURNING id").bind(case).bind(payment).bind(id).bind(cents).bind(if source=="provider" && state.cfg.demo_mode {"provider"}else{"bank_transfer"}).bind(&d.reason).bind(actor.db_id()).bind(time::fmt(state.now())).fetch_one(&mut *tx).await?;
            if source == "provider" && state.cfg.demo_mode {
                jobs::enqueue(
                    tx,
                    "finance.request_refund",
                    json!({"refund_id":refund}),
                    Some(format!("finance.refund:{refund}")),
                    state.now(),
                )
                .await?;
            }
            remaining -= cents;
        }
    }
    if remaining != 0 {
        return Err(AppError::conflict("Refundable money is already reserved by another request."));
    }
    ledger::post(
        tx,
        Some(case),
        "deposit_decision",
        id,
        "decide",
        "Bond decision: retain and reserve refund",
        &[("deposit_liability", paid, 0), ("fee_revenue", 0, retained), ("refunds_payable", 0, d.refund_cents)],
    )
    .await?;
    ledger::event(
        tx,
        actor,
        case,
        "finance.deposit_decided",
        &format!(
            "Bond decision: {} to refund, {} retained. {}",
            ledger::money(d.refund_cents),
            ledger::money(retained),
            d.reason
        ),
        json!({"decision_id":id,"retain_items":d.retain_items}),
    )
    .await?;
    ledger::tell(
        tx,
        case,
        "Your bond decision",
        &format!(
            "{} will be refunded; {} retained. {}. Refunds are awaiting confirmation.",
            ledger::money(d.refund_cents),
            ledger::money(retained),
            d.reason
        ),
    )
    .await?;
    if d.refund_cents > 0 {
        ledger::finance_notice(tx, case, "Bond refund requires confirmation").await?;
    }
    Ok(id)
}
pub async fn complete(
    tx: &mut SqliteConnection,
    actor: &Actor,
    id: i64,
    bank_reference: Option<&str>,
) -> AppResult<Option<i64>> {
    let r = sqlx::query("SELECT * FROM refunds WHERE id=?").bind(id).fetch_one(&mut *tx).await?;
    if r.get::<String, _>("status") == "completed" {
        return Ok(None);
    }
    if r.get::<String, _>("status") != "processing" {
        return Err(AppError::conflict("Only a processing refund can be confirmed."));
    }
    let method: String = r.get("method");
    if method == "bank_transfer" && bank_reference.is_none_or(|s| s.trim().is_empty()) {
        return Err(AppError::field("bank_reference", "Enter the bank transfer reference."));
    }
    let case: i64 = r.get("case_id");
    let cents: i64 = r.get("amount_cents");
    sqlx::query("UPDATE refunds SET status='completed',completed_at=?,bank_reference=?,failure_reason=NULL WHERE id=?")
        .bind(time::now_str())
        .bind(bank_reference)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    ledger::post(
        tx,
        Some(case),
        "refund",
        id,
        "complete",
        "Refund confirmed",
        &[("refunds_payable", cents, 0), (if method == "provider" { "provider_clearing" } else { "bank" }, 0, cents)],
    )
    .await?;
    let summary = format!("Refund of {} completed", ledger::money(cents));
    ledger::event(tx, actor, case, "finance.refund_completed", &summary, json!({"refund_id":id})).await?;
    ledger::tell(tx, case, "Refund completed", &summary).await?;
    Ok(Some(case))
}
pub async fn request_job(state: &AppState, id: i64) -> AppResult<()> {
    if !state.cfg.demo_mode {
        return Err(AppError::conflict("Online payment is not configured"));
    }
    let r = sqlx::query("SELECT r.*,p.external_id FROM refunds r JOIN payments p ON p.id=r.payment_id WHERE r.id=?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    if r.get::<String, _>("status") != "processing" || r.get::<Option<String>, _>("provider_refund_id").is_some() {
        return Ok(());
    }
    let response=state.http.post(format!("{}/mock/pay/api/refunds",state.cfg.internal_base_url)).header("X-Mock-Key",&state.cfg.mock_api_key).json(&json!({"payment_id":r.get::<String,_>("external_id"),"amount_cents":r.get::<i64,_>("amount_cents"),"idempotency_key":format!("refund:{id}")})).send().await.map_err(|e|AppError::internal(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::internal("DemoPay refund API unavailable"));
    }
    let v: Value = response.json().await.map_err(|e| AppError::internal(e.to_string()))?;
    let provider = v["refund_id"].as_str().ok_or_else(|| AppError::internal("DemoPay returned no refund id"))?;
    let mut tx = crate::db::write_tx(&state.db).await?;
    sqlx::query("UPDATE refunds SET provider_refund_id=? WHERE id=? AND provider_refund_id IS NULL")
        .bind(provider)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Reserve available credit immediately; confirmation uses the same refund flow as bonds.
pub async fn refund_credit(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: i64,
    cents: i64,
    reason: &str,
) -> AppResult<Vec<i64>> {
    if cents <= 0 || reason.trim().is_empty() {
        return Err(AppError::field("amount_cents", "Enter a positive amount and a refund reason."));
    }
    let sources: Vec<(i64,String,i64,i64)> = sqlx::query_as("SELECT p.id,p.source,b.credit_cents,p.amount_cents-COALESCE((SELECT SUM(amount_cents) FROM refunds WHERE payment_id=p.id),0) FROM payments p JOIN finance_payment_balances b ON b.payment_id=p.id WHERE p.case_id=? AND p.status='confirmed' AND b.credit_cents>0 ORDER BY p.id").bind(case).fetch_all(&mut *tx).await?;
    let available: i64 = sources.iter().map(|r| r.2.min(r.3)).sum();
    if cents > available {
        return Err(AppError::conflict("The refund exceeds available customer credit."));
    }
    let mut remaining = cents;
    let mut ids = vec![];
    for (payment, source, credit, available) in sources {
        let amount = remaining.min(credit).min(available);
        if amount == 0 {
            continue;
        }
        let provider = source == "provider" && state.cfg.demo_mode;
        let id: i64 = sqlx::query_scalar("INSERT INTO refunds(case_id,payment_id,amount_cents,method,status,reason,requested_by,created_at) VALUES(?,?,?,?,'processing',?,?,?) RETURNING id").bind(case).bind(payment).bind(amount).bind(if provider {"provider"} else {"bank_transfer"}).bind(reason).bind(actor.db_id()).bind(time::fmt(state.now())).fetch_one(&mut *tx).await?;
        sqlx::query("UPDATE finance_payment_balances SET credit_cents=credit_cents-? WHERE payment_id=?")
            .bind(amount)
            .bind(payment)
            .execute(&mut *tx)
            .await?;
        ledger::post(
            tx,
            Some(case),
            "refund",
            id,
            "reserve_credit",
            reason,
            &[("customer_credit", amount, 0), ("refunds_payable", 0, amount)],
        )
        .await?;
        if provider {
            jobs::enqueue(
                tx,
                "finance.request_refund",
                json!({"refund_id":id}),
                Some(format!("finance.refund:{id}")),
                state.now(),
            )
            .await?;
        }
        ids.push(id);
        remaining -= amount;
    }
    ledger::event(
        tx,
        actor,
        case,
        "finance.credit_refund_requested",
        reason,
        json!({"refund_ids":ids,"amount_cents":cents}),
    )
    .await?;
    ledger::tell(
        tx,
        case,
        "Refund processing",
        &format!("Refund of {} requested: {reason}. Awaiting confirmation.", ledger::money(cents)),
    )
    .await?;
    Ok(ids)
}
