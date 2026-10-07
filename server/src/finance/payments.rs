use super::ledger;
use crate::{
    auth::Actor,
    error::{AppError, AppResult},
    time,
};
use serde_json::json;
use sqlx::{Row, SqliteConnection};

pub async fn invoice_due(tx: &mut SqliteConnection, case: i64, invoice: i64) -> AppResult<i64> {
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM invoices WHERE id=? AND case_id=? AND kind='invoice' AND status='issued')",
    )
    .bind(invoice)
    .bind(case)
    .fetch_one(&mut *tx)
    .await?;
    if !valid {
        return Err(AppError::field("invoice_id", "Choose an issued invoice belonging to this request."));
    }
    Ok(sqlx::query_scalar("SELECT COALESCE(SUM(MAX(0,amount_cents-credited_cents-paid_cents)),0) FROM finance_line_balances WHERE invoice_id=?").bind(invoice).fetch_one(&mut *tx).await?)
}
pub(super) async fn allocate(
    tx: &mut SqliteConnection,
    actor: &Actor,
    payment: i64,
    case: i64,
    invoice: Option<i64>,
    available: i64,
) -> AppResult<i64> {
    if let Some(id) = invoice {
        invoice_due(tx, case, id).await?;
    }
    let rows:Vec<(i64,i64)>=sqlx::query_as("SELECT l.id,MAX(0,l.amount_cents-l.credited_cents-l.paid_cents) FROM finance_line_balances l JOIN invoices i ON i.id=l.invoice_id WHERE i.case_id=? AND i.kind='invoice' AND i.status='issued' AND (? IS NULL OR i.id=?) ORDER BY CASE l.kind WHEN 'fee' THEN 0 ELSE 1 END,i.id,l.id").bind(case).bind(invoice).bind(invoice).fetch_all(&mut *tx).await?;
    let mut remaining = available;
    for (line, due) in rows {
        let cents = remaining.min(due);
        if cents > 0 {
            sqlx::query("INSERT INTO payment_allocations(payment_id,invoice_line_id,amount_cents,created_by,created_at) VALUES(?,?,?,?,?)").bind(payment).bind(line).bind(cents).bind(actor.db_id()).bind(time::now_str()).execute(&mut *tx).await?;
            remaining -= cents;
        }
    }
    Ok(available - remaining)
}
#[allow(clippy::too_many_arguments)]
pub async fn receive(
    tx: &mut SqliteConnection,
    actor: &Actor,
    source: &str,
    external: &str,
    cents: i64,
    case: Option<i64>,
    invoice: Option<i64>,
    payer: Option<&str>,
    reference: Option<&str>,
) -> AppResult<(i64, bool)> {
    if cents <= 0 {
        return Err(AppError::field("amount_cents", "Enter an amount greater than zero."));
    }
    if external.trim().is_empty() {
        return Err(AppError::field("receipt_no", "A unique receipt or transaction number is required."));
    }
    let previous: Option<(i64, i64, Option<i64>)> =
        sqlx::query_as("SELECT id,amount_cents,case_id FROM payments WHERE source=? AND external_id=?")
            .bind(source)
            .bind(external)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some((id, old, old_case)) = previous {
        if old != cents || old_case != case {
            return Err(AppError::conflict("Transaction number already received with different details."));
        }
        return Ok((id, false));
    }
    if let (Some(c), Some(i)) = (case, invoice) {
        invoice_due(tx, c, i).await?;
    }
    if let Some(c) = case {
        crate::cases::core::load_case(tx, c).await?;
    }
    let id:i64=sqlx::query_scalar("INSERT INTO payments(source,external_id,amount_cents,received_at,payer_name,reference,case_id,status,recorded_by,created_at) VALUES(?,?,?,?,?,?,?,'confirmed',?,?) RETURNING id").bind(source).bind(external).bind(cents).bind(time::now_str()).bind(payer).bind(reference).bind(case).bind(actor.db_id()).bind(time::now_str()).fetch_one(&mut *tx).await?;
    let allocated = if let Some(c) = case { allocate(tx, actor, id, c, invoice, cents).await? } else { 0 };
    if let Some(c) = case {
        expire_settled_checkouts(tx, c).await?;
    }
    let credit = if case.is_some() { cents - allocated } else { 0 };
    let suspense = if case.is_none() { cents } else { 0 };
    sqlx::query("INSERT INTO finance_payment_balances(payment_id,credit_cents,suspense_cents) VALUES(?,?,?)")
        .bind(id)
        .bind(credit)
        .bind(suspense)
        .execute(&mut *tx)
        .await?;
    ledger::post(
        tx,
        case,
        "payment",
        id,
        "receive",
        "Confirmed payment received",
        &[
            (if source == "provider" { "provider_clearing" } else { "bank" }, cents, 0),
            ("receivables", 0, allocated),
            ("customer_credit", 0, credit),
            ("unallocated_receipts", 0, suspense),
        ],
    )
    .await?;
    if let Some(c) = case {
        let summary = format!(
            "Payment of {} received ({})",
            ledger::money(cents),
            if source == "provider" {
                "DemoPay"
            } else if source == "counter" {
                "Customer Care counter"
            } else {
                "bank transfer"
            }
        );
        ledger::event(tx, actor, c, "finance.payment_confirmed", &summary, json!({"payment_id":id})).await?;
        ledger::tell(tx, c, "Payment received", &summary).await?;
    } else {
        crate::audit::record(
            tx,
            actor.db_id(),
            "finance.payment_received",
            "payment",
            Some(id),
            json!({"amount_cents":cents,"unmatched":true}),
        )
        .await?;
    }
    Ok((id, true))
}
pub async fn match_suspense(
    tx: &mut SqliteConnection,
    actor: &Actor,
    payment: i64,
    case: i64,
    invoice: Option<i64>,
) -> AppResult<()> {
    let available: i64 = sqlx::query_scalar("SELECT suspense_cents FROM finance_payment_balances WHERE payment_id=?")
        .bind(payment)
        .fetch_one(&mut *tx)
        .await?;
    if available == 0 {
        return Err(AppError::conflict("This transfer has already been resolved."));
    }
    let allocated = allocate(tx, actor, payment, case, invoice, available).await?;
    sqlx::query("UPDATE finance_payment_balances SET suspense_cents=0,credit_cents=? WHERE payment_id=?")
        .bind(available - allocated)
        .bind(payment)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE payments SET case_id=? WHERE id=?").bind(case).bind(payment).execute(&mut *tx).await?;
    expire_settled_checkouts(tx, case).await?;
    let history: i64 = sqlx::query_scalar("INSERT INTO payment_match_history(payment_id,case_id,kind,reason,changed_by,changed_at) VALUES(?,?,'match','Transfer matched',?,?) RETURNING id").bind(payment).bind(case).bind(actor.db_id()).bind(time::now_str()).fetch_one(&mut *tx).await?;
    ledger::post(
        tx,
        Some(case),
        "payment_match",
        history,
        "match",
        "Bank transfer matched to request",
        &[
            ("unallocated_receipts", available, 0),
            ("receivables", 0, allocated),
            ("customer_credit", 0, available - allocated),
        ],
    )
    .await?;
    ledger::event(
        tx,
        actor,
        case,
        "finance.transfer_matched",
        "Bank transfer matched to this request",
        json!({"payment_id":payment,"allocated_cents":allocated,"customer_credit_cents":available-allocated}),
    )
    .await?;
    ledger::tell(
        tx,
        case,
        "Bank payment confirmed",
        &format!("Bank transfer of {} matched to your request.", ledger::money(available)),
    )
    .await?;
    Ok(())
}
pub async fn reverse(tx: &mut SqliteConnection, actor: &Actor, id: i64, reason: &str) -> AppResult<i64> {
    if reason.trim().is_empty() {
        return Err(AppError::field("reason", "Explain why this allocation is being reversed."));
    }
    let r=sqlx::query("SELECT a.*,i.case_id FROM payment_allocations a JOIN invoice_lines l ON l.id=a.invoice_line_id JOIN invoices i ON i.id=l.invoice_id WHERE a.id=?").bind(id).fetch_one(&mut *tx).await?;
    let payment: i64 = r.get("payment_id");
    let case: i64 = r.get("case_id");
    let cents: i64 = r.get("amount_cents");
    if r.get::<Option<String>, _>("reversed_at").is_some() {
        return Err(AppError::conflict("Allocation has already been reversed."));
    }
    let decided: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM deposit_decisions WHERE invoice_line_id=?)")
        .bind(r.get::<i64, _>("invoice_line_id"))
        .fetch_one(&mut *tx)
        .await?;
    if decided {
        return Err(AppError::conflict("An allocation funding a decided bond cannot be reversed."));
    }
    sqlx::query("UPDATE payment_allocations SET reversed_at=?,reversed_by=?,reversal_reason=? WHERE id=?")
        .bind(time::now_str())
        .bind(actor.db_id())
        .bind(reason)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE finance_payment_balances SET credit_cents=credit_cents+? WHERE payment_id=?")
        .bind(cents)
        .bind(payment)
        .execute(&mut *tx)
        .await?;
    ledger::post(
        tx,
        Some(case),
        "allocation",
        id,
        "reverse",
        reason,
        &[("receivables", cents, 0), ("customer_credit", 0, cents)],
    )
    .await?;
    ledger::event(
        tx,
        actor,
        case,
        "finance.allocation_reversed",
        &format!("Payment allocation reversed: {reason}"),
        json!({"allocation_id":id}),
    )
    .await?;
    Ok(payment)
}
pub async fn allocate_credit(
    tx: &mut SqliteConnection,
    actor: &Actor,
    payment: i64,
    case: i64,
    invoice: Option<i64>,
) -> AppResult<i64> {
    let (available,old_case):(i64,Option<i64>)=sqlx::query_as("SELECT b.credit_cents,p.case_id FROM finance_payment_balances b JOIN payments p ON p.id=b.payment_id WHERE p.id=? AND p.status='confirmed'").bind(payment).fetch_one(&mut *tx).await?;
    if old_case != Some(case) {
        let same_applicant:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cases a JOIN cases b ON b.id=? WHERE a.id=? AND ((a.applicant_org_id IS NOT NULL AND a.applicant_org_id=b.applicant_org_id) OR (a.applicant_org_id IS NULL AND b.applicant_org_id IS NULL AND a.applicant_user_id IS NOT NULL AND a.applicant_user_id=b.applicant_user_id)))").bind(case).bind(old_case).fetch_one(&mut *tx).await?;
        if !same_applicant {
            return Err(AppError::field(
                "case_id",
                "Customer credit can only be transferred between requests belonging to the same applicant.",
            ));
        }
    }
    let before: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id),0) FROM payment_allocations WHERE payment_id=?")
        .bind(payment)
        .fetch_one(&mut *tx)
        .await?;
    let allocated = allocate(tx, actor, payment, case, invoice, available).await?;
    if allocated == 0 {
        return Ok(0);
    }
    sqlx::query("UPDATE finance_payment_balances SET credit_cents=credit_cents-? WHERE payment_id=?")
        .bind(allocated)
        .bind(payment)
        .execute(&mut *tx)
        .await?;
    let last: i64 = sqlx::query_scalar("SELECT MAX(id) FROM payment_allocations WHERE payment_id=? AND id>?")
        .bind(payment)
        .bind(before)
        .fetch_one(&mut *tx)
        .await?;
    ledger::post(
        tx,
        Some(case),
        "allocation",
        last,
        "allocate_credit",
        "Customer credit allocated to charges",
        &[("customer_credit", allocated, 0), ("receivables", 0, allocated)],
    )
    .await?;
    ledger::event(
        tx,
        actor,
        case,
        "finance.credit_allocated",
        "Available customer credit applied to charges",
        json!({"payment_id":payment,"amount_cents":allocated}),
    )
    .await?;
    Ok(allocated)
}

async fn expire_settled_checkouts(tx: &mut SqliteConnection, case: i64) -> AppResult<()> {
    sqlx::query("UPDATE checkout_sessions SET status='expired',completed_at=? WHERE case_id=? AND status='open' AND NOT EXISTS(SELECT 1 FROM finance_line_balances l WHERE l.invoice_id=checkout_sessions.invoice_id AND l.amount_cents-l.credited_cents-l.paid_cents>0)")
        .bind(time::now_str()).bind(case).execute(&mut *tx).await?;
    Ok(())
}

/// A bank receipt can return to suspense before matching it to a different applicant.
pub async fn unmatch(tx: &mut SqliteConnection, actor: &Actor, id: i64, reason: &str) -> AppResult<i64> {
    if reason.trim().is_empty() {
        return Err(AppError::field("reason", "Explain the correction."));
    }
    let (source, case): (String, Option<i64>) =
        sqlx::query_as("SELECT source,case_id FROM payments WHERE id=? AND status='confirmed'")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    let case = case.ok_or_else(|| AppError::conflict("This receipt is already in suspense."))?;
    if source != "bank_transfer" {
        return Err(AppError::conflict("Only bank transfers can be unmatched."));
    }
    let reserved: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM refunds WHERE payment_id=?)")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if reserved {
        return Err(AppError::conflict("A refunded or reserved payment cannot be unmatched."));
    }
    let allocations: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM payment_allocations WHERE payment_id=? AND reversed_at IS NULL")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    for allocation in allocations {
        reverse(tx, actor, allocation, reason).await?;
    }
    let credit: i64 = sqlx::query_scalar("SELECT credit_cents FROM finance_payment_balances WHERE payment_id=?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("UPDATE finance_payment_balances SET credit_cents=0,suspense_cents=? WHERE payment_id=?")
        .bind(credit)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE payments SET case_id=NULL WHERE id=?").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE statement_rows SET status='unmatched',suggested_case_id=NULL,resolved_by=NULL,resolved_at=NULL WHERE payment_id=?").bind(id).execute(&mut *tx).await?;
    let history: i64 = sqlx::query_scalar("INSERT INTO payment_match_history(payment_id,case_id,kind,reason,changed_by,changed_at) VALUES(?,?,'unmatch',?,?,?) RETURNING id").bind(id).bind(case).bind(reason).bind(actor.db_id()).bind(time::now_str()).fetch_one(&mut *tx).await?;
    ledger::post(
        tx,
        Some(case),
        "payment_match",
        history,
        "unmatch",
        reason,
        &[("customer_credit", credit, 0), ("unallocated_receipts", 0, credit)],
    )
    .await?;
    ledger::event(tx, actor, case, "finance.payment_unmatched", reason, json!({"payment_id":id,"amount_cents":credit}))
        .await?;
    ledger::tell(
        tx,
        case,
        "Bank transfer correction",
        "A wrongly matched transfer was returned to suspense. The updated balance is shown on your request.",
    )
    .await?;
    Ok(case)
}
