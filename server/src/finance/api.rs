// OWNER: finance
//! Cross-module finance API. Money is integer cents (AUD).

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqliteConnection;

use crate::auth::Actor;
use crate::cases::core::CaseRow;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// A priced line, ready to become an `invoice_lines` row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuoteLine {
    pub price_item_id: Option<i64>,
    pub price_version_id: Option<i64>,
    pub item_code: String,
    /// `fee` | `deposit`.
    pub kind: String,
    pub description: String,
    /// 1000 = 1 unit.
    pub quantity_milli: i64,
    /// Hourly lines: exact minutes (amount = round_half_up(minutes × rate / 60)).
    pub quantity_minutes: Option<i64>,
    pub unit_amount_cents: i64,
    pub amount_cents: i64,
    /// Inputs used, e.g. `{"billable_minutes":330,"source":"equipment_usage#3"}`.
    pub calc: Value,
}

/// Money position of a case.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoneySummary {
    pub invoiced_cents: i64,
    pub credited_cents: i64,
    pub paid_cents: i64,
    pub outstanding_cents: i64,
    pub deposits_held_cents: i64,
    pub refunded_cents: i64,
    pub settled: bool,
}

use super::{ledger, payments};
use crate::{
    cases::core::{self, Visibility},
    time,
};
use serde_json::json;
use sqlx::Row;

pub async fn quote(
    tx: &mut SqliteConnection,
    item_code: &str,
    quantity_milli: i64,
    quantity_minutes: Option<i64>,
    pricing_date: NaiveDate,
) -> AppResult<QuoteLine> {
    if quantity_milli < 0 || quantity_minutes.is_some_and(|m| m < 0) {
        return Err(AppError::field("quantity_milli", "Quantities and actual minutes cannot be negative."));
    }
    let r = sqlx::query("SELECT i.id item_id,i.name,i.kind,v.id version_id,v.amount_cents FROM price_items i JOIN price_versions v ON v.price_item_id=i.id WHERE i.code=? AND v.effective_from<=? AND (v.effective_to IS NULL OR v.effective_to>?)")
        .bind(item_code).bind(pricing_date.to_string()).bind(pricing_date.to_string()).fetch_optional(&mut *tx).await?.ok_or_else(|| AppError::field("item",format!("No price for {item_code} on {pricing_date}.")))?;
    let rate: i64 = r.get("amount_cents");
    let amount_cents = ledger::amount(
        quantity_minutes.unwrap_or(quantity_milli),
        rate,
        if quantity_minutes.is_some() { 60 } else { 1000 },
    )?;
    Ok(QuoteLine {
        price_item_id: Some(r.get("item_id")),
        price_version_id: Some(r.get("version_id")),
        item_code: item_code.into(),
        kind: r.get("kind"),
        description: r.get("name"),
        quantity_milli,
        quantity_minutes,
        unit_amount_cents: rate,
        amount_cents,
        calc: json!({"quantity_minutes":quantity_minutes}),
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn persist_invoice(
    tx: &mut SqliteConnection,
    actor: &Actor,
    case: i64,
    kind: &str,
    date: NaiveDate,
    lines: &[QuoteLine],
    note: Option<&str>,
    credits: Option<i64>,
) -> AppResult<i64> {
    let prefix = match kind {
        "estimate" => "EST",
        "invoice" => "INV",
        "credit_note" => "CN",
        _ => return Err(AppError::field("kind", "Choose estimate, invoice or credit_note.")),
    };
    let mut total = 0i64;
    for l in lines {
        if !matches!(l.kind.as_str(), "fee" | "deposit")
            || l.description.trim().is_empty()
            || l.amount_cents
                != ledger::amount(
                    l.quantity_minutes.unwrap_or(l.quantity_milli),
                    l.unit_amount_cents,
                    if l.quantity_minutes.is_some() { 60 } else { 1000 },
                )?
        {
            return Err(AppError::field("lines", "Each line needs a description and an exact integer amount."));
        }
        total = total.checked_add(l.amount_cents).ok_or_else(|| AppError::field("lines", "Total is too large."))?;
    }
    let year = time::to_local(chrono::Utc::now()).format("%Y");
    let stem = format!("{prefix}-{year}-");
    let seq: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(CAST(substr(number,-5) AS INTEGER)),0)+1 FROM invoices WHERE number LIKE ?",
    )
    .bind(format!("{stem}%"))
    .fetch_one(&mut *tx)
    .await?;
    if seq > 99999 {
        return Err(AppError::conflict("Invoice sequence is exhausted for this year."));
    }
    let number = format!("{stem}{seq:05}");
    let id:i64 = sqlx::query_scalar("INSERT INTO invoices(case_id,number,kind,status,pricing_date,total_cents,basis_note,credits_invoice_id,created_by,created_at,issued_at) VALUES(?,?,?,'issued',?,?,?,?,?,?,?) RETURNING id")
        .bind(case).bind(&number).bind(kind).bind(date.to_string()).bind(total).bind(note).bind(credits).bind(actor.db_id()).bind(time::now_str()).bind(time::now_str()).fetch_one(&mut *tx).await?;
    for l in lines {
        sqlx::query("INSERT INTO invoice_lines(invoice_id,price_item_id,price_version_id,kind,description,quantity_milli,unit_amount_cents,amount_cents,calc_json,quantity_minutes) VALUES(?,?,?,?,?,?,?,?,?,?)")
        .bind(id).bind(l.price_item_id).bind(l.price_version_id).bind(&l.kind).bind(&l.description).bind(l.quantity_milli).bind(l.unit_amount_cents).bind(l.amount_cents).bind(l.calc.to_string()).bind(l.quantity_minutes).execute(&mut *tx).await?;
    }
    if kind != "estimate" {
        let fees: i64 = lines.iter().filter(|l| l.kind == "fee").map(|l| l.amount_cents).sum();
        let deposits = total - fees;
        let posting = if kind == "invoice" {
            vec![("receivables", total, 0), ("fee_revenue", 0, fees), ("deposit_liability", 0, deposits)]
        } else {
            vec![("receivables", 0, total), ("fee_revenue", fees, 0), ("deposit_liability", deposits, 0)]
        };
        ledger::post(tx, Some(case), kind, id, "issue", &format!("{number} issued"), &posting).await?;
    }
    ledger::event(
        tx,
        actor,
        case,
        "finance.invoice_issued",
        &format!("{number} issued for {}", ledger::money(total)),
        json!({"invoice_id":id,"kind":kind}),
    )
    .await?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
pub async fn issue_invoice(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case_id: i64,
    kind: &str,
    pricing_date: NaiveDate,
    lines: Vec<QuoteLine>,
    basis_note: Option<String>,
) -> AppResult<i64> {
    let credits = if kind == "credit_note" {
        let mut original = None;
        let mut credited_lines = std::collections::BTreeSet::new();
        if lines.is_empty() {
            return Err(AppError::field("lines", "Select the original invoice lines to credit."));
        }
        for line in &lines {
            let line_id = line.calc["original_line_id"]
                .as_i64()
                .ok_or_else(|| AppError::field("lines", "Credit lines must identify original_line_id in calc."))?;
            if !credited_lines.insert(line_id) {
                return Err(AppError::field("lines", "Credit each original invoice line only once."));
            }
            let (invoice,available,line_kind):(i64,i64,String)=sqlx::query_as("SELECT l.invoice_id,l.amount_cents-l.credited_cents-l.paid_cents,l.kind FROM finance_line_balances l JOIN invoices i ON i.id=l.invoice_id WHERE l.id=? AND i.case_id=? AND i.kind='invoice' AND i.status='issued'").bind(line_id).bind(case_id).fetch_one(&mut *tx).await?;
            if original.is_some_and(|id| id != invoice) || line.amount_cents > available || line.kind != line_kind {
                return Err(AppError::field(
                    "lines",
                    "Credit one original invoice at a time, after reversing any allocations on the credited lines.",
                ));
            }
            original = Some(invoice);
        }
        original
    } else {
        None
    };
    let id = persist_invoice(tx, actor, case_id, kind, pricing_date, &lines, basis_note.as_deref(), credits).await?;
    attach_pdf(tx, state, actor, id).await?;
    if kind == "invoice" {
        ledger::tell(
            tx,
            case_id,
            "Invoice ready",
            "Your invoice is ready. Open your request to see the charges and payment options.",
        )
        .await?;
    }
    Ok(id)
}
pub(super) async fn attach_pdf(tx: &mut SqliteConnection, state: &AppState, actor: &Actor, id: i64) -> AppResult<()> {
    let (case, number, kind, note, total): (i64, String, String, Option<String>, i64) =
        sqlx::query_as("SELECT case_id,number,kind,basis_note,total_cents FROM invoices WHERE id=?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    let rows: Vec<(String, String, i64)> =
        sqlx::query_as("SELECT description,kind,amount_cents FROM invoice_lines WHERE invoice_id=?")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    let body = rows
        .into_iter()
        .map(|(d, k, a)| {
            format!("{}: {}{}", d, ledger::money(a), if k == "deposit" { " — Refundable bond" } else { "" })
        })
        .collect::<Vec<_>>()
        .join("\n");
    let bytes = crate::pdf::simple_document(
        &number,
        &[("Total", ledger::money(total))],
        &[("FY2026-27 schedule (demo copy — confirm with Council)", body), ("Basis", note.unwrap_or_default())],
    );
    let (doc, version) = crate::documents::api::attach_generated(
        tx,
        state,
        case,
        if kind == "estimate" { "invoice" } else { &kind },
        &number,
        Visibility::Applicant,
        bytes,
        actor.db_id(),
    )
    .await?;
    sqlx::query("INSERT INTO finance_invoice_documents(invoice_id,document_id,document_version_id) VALUES(?,?,?)")
        .bind(id)
        .bind(doc)
        .bind(version)
        .execute(&mut *tx)
        .await?;
    Ok(())
}
pub async fn ensure_invoice_for_step(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: &CaseRow,
) -> AppResult<Option<i64>> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM invoices i WHERE case_id=? AND kind='invoice' AND status='issued')",
    )
    .bind(case.id)
    .fetch_one(&mut *tx)
    .await?;
    // A paid invoice also covers this payment step; do not charge again on re-entry.
    if exists {
        return Ok(None);
    }
    let (date, lines) = crate::hooks::pricing_lines(tx, case).await?;
    if lines.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        issue_invoice(
            tx,
            state,
            actor,
            case.id,
            "invoice",
            date,
            lines,
            Some("Priced from the submitted request.".into()),
        )
        .await?,
    ))
}
pub async fn definition_pricing_lines(tx: &mut SqliteConnection, case: &CaseRow) -> AppResult<Vec<QuoteLine>> {
    // Read JSON decimals as text: monetary arithmetic never goes through f64.
    let definition: String=sqlx::query_scalar("SELECT COALESCE((SELECT definition_snapshot_json FROM submissions WHERE case_id=?),(SELECT definition_json FROM service_versions WHERE id=?))").bind(case.id).bind(case.service_version_id).fetch_one(&mut *tx).await?;
    let v: Value = serde_json::from_str(&definition)?;
    let date = time::local_date(time::parse(case.submitted_at.as_deref().unwrap_or(&case.created_at))?);
    let mut lines = Vec::new();
    for p in v["pricing"].as_array().into_iter().flatten() {
        let qty = super::statements::decimal_units(
            &p.get("quantity").map(Value::to_string).unwrap_or_else(|| "1".into()),
            3,
        )?;
        lines.push(
            quote(
                tx,
                p["item"].as_str().ok_or_else(|| AppError::field("pricing", "A price item is required."))?,
                qty,
                None,
                date,
            )
            .await?,
        );
    }
    Ok(lines)
}
pub async fn case_settled(tx: &mut SqliteConnection, case_id: i64) -> AppResult<bool> {
    let owing:i64=sqlx::query_scalar("SELECT COUNT(*) FROM finance_line_balances l JOIN invoices i ON i.id=l.invoice_id WHERE i.case_id=? AND i.kind='invoice' AND i.status='issued' AND l.amount_cents-l.credited_cents-l.paid_cents>0").bind(case_id).fetch_one(&mut *tx).await?;
    Ok(owing == 0)
}
pub async fn deposits_settled(tx: &mut SqliteConnection, case_id: i64) -> AppResult<bool> {
    let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM finance_line_balances l JOIN invoices i ON i.id=l.invoice_id LEFT JOIN deposit_decisions d ON d.invoice_line_id=l.id WHERE i.case_id=? AND i.kind='invoice' AND i.status='issued' AND l.kind='deposit' AND l.amount_cents>l.credited_cents AND (d.id IS NULL OR l.paid_cents<l.amount_cents-l.credited_cents OR EXISTS(SELECT 1 FROM refunds r WHERE r.deposit_decision_id=d.id AND r.status<>'completed')))").bind(case_id).fetch_one(&mut *tx).await?;
    Ok(!pending)
}
pub async fn case_money_summary(tx: &mut SqliteConnection, case_id: i64) -> AppResult<MoneySummary> {
    let (invoiced,credited,paid,outstanding,deposits):(i64,i64,i64,i64,i64)=sqlx::query_as("SELECT COALESCE(SUM(l.amount_cents),0),COALESCE(SUM(l.credited_cents),0),COALESCE(SUM(l.paid_cents),0),COALESCE(SUM(MAX(0,l.amount_cents-l.credited_cents-l.paid_cents)),0),COALESCE(SUM(CASE WHEN l.kind='deposit' THEN l.paid_cents ELSE 0 END),0) FROM finance_line_balances l JOIN invoices i ON i.id=l.invoice_id WHERE i.case_id=? AND i.kind='invoice' AND i.status='issued'").bind(case_id).fetch_one(&mut *tx).await?;
    let (retained,refunds):(i64,i64)=sqlx::query_as("SELECT (SELECT COALESCE(SUM(retain_cents),0) FROM deposit_decisions WHERE case_id=?),(SELECT COALESCE(SUM(amount_cents),0) FROM refunds WHERE case_id=? AND status='completed')").bind(case_id).bind(case_id).fetch_one(&mut *tx).await?;
    Ok(MoneySummary {
        invoiced_cents: invoiced,
        credited_cents: credited,
        paid_cents: paid,
        outstanding_cents: outstanding,
        deposits_held_cents: deposits - retained - refunds,
        refunded_cents: refunds,
        settled: outstanding == 0,
    })
}
#[allow(clippy::too_many_arguments)]
pub async fn reprice_case(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case_id: i64,
    pricing_date: NaiveDate,
    new_lines: Vec<QuoteLine>,
    note: &str,
) -> AppResult<()> {
    let ids = reprice_entries(tx, state, actor, case_id, pricing_date, new_lines, note).await?;
    for id in ids {
        attach_pdf(tx, state, actor, id).await?;
    }
    ledger::tell(tx,case_id,"Booking charges revised","Your booking charges have been revised. Open your request to see the credit notes, revised invoice and any customer credit.").await?;
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub(super) async fn reprice_entries(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case_id: i64,
    pricing_date: NaiveDate,
    mut new_lines: Vec<QuoteLine>,
    note: &str,
) -> AppResult<Vec<i64>> {
    require_unconsumed(tx, state, case_id).await?;
    let old: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM invoices WHERE case_id=? AND kind='invoice' AND status='issued'")
            .bind(case_id)
            .fetch_all(&mut *tx)
            .await?;
    let mut issued_ids = Vec::new();
    for invoice in old {
        let rows =
            sqlx::query("SELECT * FROM finance_line_balances WHERE invoice_id=? AND amount_cents>credited_cents")
                .bind(invoice)
                .fetch_all(&mut *tx)
                .await?;
        let mut credits = Vec::new();
        for r in rows {
            // An unchanged refundable bond remains allocated to its original invoice line.
            if r.get::<String, _>("kind") == "deposit"
                && r.get::<i64, _>("credited_cents") == 0
                && let Some(index) = new_lines.iter().position(|l| {
                    l.kind == "deposit"
                        && l.price_item_id == r.get::<Option<i64>, _>("price_item_id")
                        && l.description == r.get::<String, _>("description")
                        && l.amount_cents == r.get::<i64, _>("amount_cents")
                        && l.quantity_milli == r.get::<i64, _>("quantity_milli")
                        && l.unit_amount_cents == r.get::<i64, _>("unit_amount_cents")
                })
            {
                new_lines.remove(index);
                continue;
            }
            let line: i64 = r.get("id");
            let allocations: Vec<i64> = sqlx::query_scalar(
                "SELECT id FROM payment_allocations WHERE invoice_line_id=? AND reversed_at IS NULL",
            )
            .bind(line)
            .fetch_all(&mut *tx)
            .await?;
            for a in allocations {
                payments::reverse(tx, actor, a, "Booking rescheduled; transfer available money to revised charges")
                    .await?;
            }
            let cents = r.get::<i64, _>("amount_cents") - r.get::<i64, _>("credited_cents");
            credits.push(QuoteLine {
                price_item_id: r.get("price_item_id"),
                price_version_id: r.get("price_version_id"),
                item_code: String::new(),
                kind: r.get("kind"),
                description: r.get("description"),
                quantity_milli: 1000,
                quantity_minutes: None,
                unit_amount_cents: cents,
                amount_cents: cents,
                calc: json!({"original_line_id":line}),
            });
        }
        if !credits.is_empty() {
            let cn =
                persist_invoice(tx, actor, case_id, "credit_note", pricing_date, &credits, Some(note), Some(invoice))
                    .await?;
            issued_ids.push(cn);
        }
    }
    if !new_lines.is_empty() {
        let revised=persist_invoice(tx,actor,case_id,"invoice",pricing_date,&new_lines,Some(&format!("{note}. Unused old charges credited. Unchanged bonds remain on their original invoice lines. Received money applied to revised charges, fees first; any surplus remains customer credit.")),None).await?;
        issued_ids.push(revised);
    }
    let credits:Vec<i64>=sqlx::query_scalar("SELECT p.id FROM payments p JOIN finance_payment_balances b ON b.payment_id=p.id WHERE p.case_id=? AND b.credit_cents>0 AND p.status='confirmed'").bind(case_id).fetch_all(&mut *tx).await?;
    for payment in credits {
        payments::allocate_credit(tx, actor, payment, case_id, None).await?;
    }
    core::bump_revision(tx, case_id, None).await?;
    Ok(issued_ids)
}

pub async fn require_unconsumed(tx: &mut SqliteConnection, state: &AppState, case_id: i64) -> AppResult<()> {
    let consumed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM deposit_decisions WHERE case_id=?) OR EXISTS(SELECT 1 FROM bookings WHERE case_id=? AND (status='completed' OR end_at<=?)) OR EXISTS(SELECT 1 FROM equipment_usage u JOIN equipment_requests r ON r.id=u.equipment_request_id WHERE r.case_id=? AND u.approved_at IS NOT NULL)").bind(case_id).bind(case_id).bind(time::fmt(state.now())).bind(case_id).fetch_one(&mut *tx).await?;
    if consumed {
        return Err(AppError::conflict("Consumed hire charges and decided bonds cannot be repriced."));
    }
    Ok(())
}
