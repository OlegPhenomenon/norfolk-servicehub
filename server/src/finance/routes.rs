use super::{deposits, ledger, payments, prices, statements, views};
use crate::{
    auth::Actor,
    authz::{self, CaseAccess, Role},
    cases::core,
    db,
    error::{AppError, AppResult},
    idempotency::{self, IdempotencyKey},
    state::AppState,
    time,
    web::{Json, Path, Query},
};
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/prices", get(price_list))
        .route("/api/admin/prices/{code}/versions", post(price_version))
        .route("/api/cases/{id}/money", get(money))
        .route("/api/cases/{id}/checkout", post(checkout))
        .route(
            "/api/webhooks/demopay",
            post(super::webhooks::handler)
                .layer::<_, std::convert::Infallible>(axum::extract::DefaultBodyLimit::max(
                    super::webhooks::MAX_WEBHOOK_BYTES,
                ))
                .layer(tower_http::limit::RequestBodyLimitLayer::new(super::webhooks::MAX_WEBHOOK_BYTES)),
        )
        .route("/api/finance/overview", get(overview))
        .route("/api/finance/statements", post(import_statement))
        .route("/api/finance/unmatched", get(unmatched))
        .route("/api/finance/cases", get(case_search))
        .route("/api/finance/statement-rows/{id}/match", post(match_row))
        .route("/api/finance/statement-rows/{id}/ignore", post(ignore_row))
        .route("/api/finance/allocations/{id}/reverse", post(reverse))
        .route("/api/finance/payments/{id}/allocate", post(allocate))
        .route("/api/finance/payments/counter", post(counter))
        .route("/api/finance/deposits", get(deposit_queue))
        .route("/api/cases/{id}/refund-credit", post(refund_credit))
        .route("/api/cases/{id}/price-waivers", post(waiver))
        .route("/api/finance/payments/{id}/unmatch", post(unmatch))
        .route("/api/cases/{id}/deposit-decision", post(deposit_decision))
        .route("/api/finance/refunds", get(refund_queue))
        .route("/api/finance/refunds/{id}/confirm-bank", post(confirm_bank))
        .route("/api/finance/refunds/{id}/retry-bank", post(retry_bank))
        .route("/api/finance/ledger", get(journal))
}
fn finance(actor: &Actor) -> AppResult<()> {
    actor.require_any_role(&[Role::Finance])
}
async fn readable(tx: &mut SqliteConnection, actor: &Actor, id: i64) -> AppResult<bool> {
    let (_, access) = authz::require_case(tx, actor, id).await?;
    if matches!(access, CaseAccess::TaskOnly) {
        return Err(AppError::not_found());
    }
    Ok(access.is_staff())
}
async fn manageable(tx: &mut SqliteConnection, actor: &Actor, id: i64, revision: Option<i64>) -> AppResult<()> {
    finance(actor)?;
    authz::require_staff_case(tx, actor, id).await?;
    let rev =
        revision.ok_or_else(|| AppError::field("expected_revision", "Reload this request before making changes."))?;
    core::bump_revision(tx, id, Some(rev)).await?;
    Ok(())
}
async fn confirmed(tx: &mut SqliteConnection, state: &AppState, payment: i64, case: i64) -> AppResult<()> {
    crate::records::api::on_payment_confirmed(tx, payment).await?;
    crate::cases::workflow::try_auto_advance(tx, state, case).await
}
async fn price_list(State(s): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    actor.require_any_role(&[Role::Finance, Role::Sysadmin])?;
    let mut c = s.db.acquire().await?;
    Ok(Json(prices::list(&mut c).await?))
}
#[derive(Deserialize)]
struct NewPrice {
    amount_cents: i64,
    effective_from: String,
}
async fn price_version(
    State(s): State<AppState>,
    actor: Actor,
    Path(code): Path<String>,
    Json(b): Json<NewPrice>,
) -> AppResult<StatusCode> {
    actor.require_any_role(&[Role::Finance, Role::Sysadmin])?;
    let mut tx = db::write_tx(&s.db).await?;
    prices::schedule(&mut tx, &s, &actor, &code, b.amount_cents, &b.effective_from).await?;
    tx.commit().await?;
    Ok(StatusCode::CREATED)
}
async fn money(State(s): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut c = s.db.acquire().await?;
    let staff = readable(&mut c, &actor, id).await?;
    let mut data = views::case_money(&mut c, id, staff, &time::fmt(s.now())).await?;
    data["online_payment_enabled"] = json!(s.cfg.demo_mode);
    let case = core::load_case(&mut c, id).await?;
    if staff
        && actor.roles_for_service(case.service_id).contains(&Role::Manager)
        && !data["invoices"].as_array().is_some_and(|items| items.iter().any(|i| i["kind"] == "invoice"))
        && let Ok((_, lines)) = crate::hooks::pricing_lines(&mut c, &case).await
    {
        data["waiver_quotes"] =
            json!(lines.into_iter().filter(|l| l.kind == "fee" && l.amount_cents > 0).collect::<Vec<_>>());
    }
    Ok(Json(data))
}
#[derive(Deserialize)]
struct Checkout {
    invoice_id: i64,
}
async fn checkout(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<Checkout>,
) -> AppResult<Json<Value>> {
    if !s.cfg.demo_mode {
        return Err(AppError::conflict("Online payment is not configured"));
    }
    let (amount, number) = {
        let mut c = s.db.acquire().await?;
        readable(&mut c, &actor, id).await?;
        let due = payments::invoice_due(&mut c, id, b.invoice_id).await?;
        let n: String =
            sqlx::query_scalar("SELECT number FROM invoices WHERE id=?").bind(b.invoice_id).fetch_one(&mut *c).await?;
        (due, n)
    };
    if amount <= 0 {
        return Err(AppError::conflict("This invoice has already been settled."));
    }
    let url = format!("{}/my/cases/{id}?tab=finance.money&paid=1", s.cfg.public_base_url);
    let res=s.http.post(format!("{}/mock/pay/api/sessions",s.cfg.internal_base_url)).header("X-Mock-Key",&s.cfg.mock_api_key).json(&json!({"amount_cents":amount,"currency":"AUD","reference":number,"return_url":url,"metadata":{"case_id":id,"invoice_id":b.invoice_id}})).send().await.map_err(|e|AppError::internal(e.to_string()))?;
    if !res.status().is_success() {
        return Err(AppError::internal("DemoPay checkout could not be created"));
    }
    let provider: Value = res.json().await.map_err(|e| AppError::internal(e.to_string()))?;
    let session = provider["session_id"].as_str().ok_or_else(|| AppError::internal("Missing provider session"))?;
    let mut tx = db::write_tx(&s.db).await?;
    readable(&mut tx, &actor, id).await?;
    payments::invoice_due(&mut tx, id, b.invoice_id).await?;
    sqlx::query("INSERT INTO checkout_sessions(provider_session_id,case_id,invoice_id,amount_cents,status,created_by,created_at) VALUES(?,?,?,?,'open',?,?)").bind(session).bind(id).bind(b.invoice_id).bind(amount).bind(actor.db_id()).bind(time::fmt(s.now())).execute(&mut *tx).await?;
    ledger::event(
        &mut tx,
        &actor,
        id,
        "finance.checkout_created",
        "DemoPay checkout opened; payment awaiting provider confirmation",
        json!({"session_id":session}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"checkout_url":provider["checkout_url"]})))
}
async fn overview(State(s): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    finance(&actor)?;
    let mut c = s.db.acquire().await?;
    Ok(Json(views::overview(&mut c, &actor, &time::fmt(s.now())).await?))
}
async fn unmatched(State(s): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    finance(&actor)?;
    let mut c = s.db.acquire().await?;
    let rows = views::queue(&mut c, &actor, "unmatched", &time::fmt(s.now())).await?;
    let evidence=views::scoped(&mut c,&actor,"SELECT d.id,d.title,d.case_id,c.number case_number,d.created_at FROM documents d JOIN cases c ON c.id=d.case_id WHERE {scope} AND d.category='receipt' AND d.disposed_at IS NULL ORDER BY d.id DESC LIMIT 100").await?;
    Ok(Json(
        json!({"rows":rows,"evidence":evidence,"evidence_note":"Evidence only — not money. Uploaded receipts never create payments."}),
    ))
}
#[derive(Deserialize)]
struct Statement {
    filename: String,
    csv: String,
}
async fn import_statement(State(s): State<AppState>, actor: Actor, Json(b): Json<Statement>) -> AppResult<Json<Value>> {
    finance(&actor)?;
    let mut tx = db::write_tx(&s.db).await?;
    let (report, receipts) = statements::import(&mut tx, &s, &actor, &b.filename, &b.csv).await?;
    for (payment, case) in receipts {
        confirmed(&mut tx, &s, payment, case).await?;
    }
    tx.commit().await?;
    Ok(Json(report))
}
#[derive(Deserialize)]
struct CaseSearch {
    #[serde(default)]
    q: String,
}
async fn case_search(State(s): State<AppState>, actor: Actor, Query(b): Query<CaseSearch>) -> AppResult<Json<Value>> {
    finance(&actor)?;
    let mut c = s.db.acquire().await?;
    let rows=views::scoped(&mut c,&actor,"SELECT c.id,c.number,c.applicant_name,c.title,c.revision FROM cases c WHERE {scope} AND c.number IS NOT NULL ORDER BY c.updated_at DESC").await?;
    let term = b.q.to_lowercase();
    let mut matches = Vec::new();
    for mut r in rows
        .into_iter()
        .filter(|r| {
            ["number", "applicant_name", "title"]
                .iter()
                .any(|key| r[key].as_str().unwrap_or_default().to_lowercase().contains(&term))
        })
        .take(30)
    {
        let invoices=sqlx::query("SELECT i.id,i.number,COALESCE(SUM(MAX(0,l.amount_cents-l.credited_cents-l.paid_cents)),0) outstanding_cents FROM invoices i LEFT JOIN finance_line_balances l ON l.invoice_id=i.id WHERE i.case_id=? AND i.kind='invoice' AND i.status='issued' GROUP BY i.id").bind(r["id"].as_i64()).fetch_all(&mut *c).await?;
        r["invoices"] = json!(invoices.iter().map(views::row).collect::<Vec<_>>());
        matches.push(r);
    }
    Ok(Json(json!(matches)))
}
#[derive(Deserialize, Serialize)]
struct Target {
    case_id: i64,
    invoice_id: Option<i64>,
    expected_revision: Option<i64>,
}
async fn match_row(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<Target>,
) -> AppResult<StatusCode> {
    let mut tx = db::write_tx(&s.db).await?;
    manageable(&mut tx, &actor, b.case_id, b.expected_revision).await?;
    let (payment, suggested): (i64, Option<i64>) =
        sqlx::query_as("SELECT payment_id,suggested_case_id FROM statement_rows WHERE id=? AND status='unmatched'")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    if let Some(c) = suggested {
        readable(&mut tx, &actor, c).await?;
    }
    payments::match_suspense(&mut tx, &actor, payment, b.case_id, b.invoice_id).await?;
    sqlx::query("UPDATE statement_rows SET status='matched',resolved_by=?,resolved_at=? WHERE id=?")
        .bind(actor.db_id())
        .bind(time::fmt(s.now()))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    confirmed(&mut tx, &s, payment, b.case_id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
struct Note {
    note: String,
}
async fn ignore_row(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<Note>,
) -> AppResult<StatusCode> {
    finance(&actor)?;
    let mut tx = db::write_tx(&s.db).await?;
    let case: Option<i64> = sqlx::query_scalar("SELECT suggested_case_id FROM statement_rows WHERE id=?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if let Some(c) = case {
        readable(&mut tx, &actor, c).await?;
        core::append_event(
            &mut tx,
            c,
            actor.db_id(),
            "finance.transfer_ignored",
            core::Visibility::Staff,
            &format!("Suggested transfer ignored: {}", b.note),
            json!({"row_id":id}),
        )
        .await?;
    }
    statements::ignore(&mut tx, &actor, id, &b.note).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
struct Reason {
    reason: String,
    expected_revision: Option<i64>,
}
async fn reverse(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<Reason>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&s.db).await?;
    let case:i64=sqlx::query_scalar("SELECT i.case_id FROM payment_allocations a JOIN invoice_lines l ON l.id=a.invoice_line_id JOIN invoices i ON i.id=l.invoice_id WHERE a.id=?").bind(id).fetch_one(&mut *tx).await?;
    manageable(&mut tx, &actor, case, b.expected_revision).await?;
    let payment = payments::reverse(&mut tx, &actor, id, &b.reason).await?;
    ledger::tell(
        &mut tx,
        case,
        "Payment allocation corrected",
        "A payment allocation has been corrected. Open your request to see the updated balance.",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"payment_id":payment})))
}
async fn allocate(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<Target>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&s.db).await?;
    manageable(&mut tx, &actor, b.case_id, b.expected_revision).await?;
    let old: Option<i64> =
        sqlx::query_scalar("SELECT case_id FROM payments WHERE id=?").bind(id).fetch_one(&mut *tx).await?;
    if let Some(c) = old {
        readable(&mut tx, &actor, c).await?;
    }
    let amount = payments::allocate_credit(&mut tx, &actor, id, b.case_id, b.invoice_id).await?;
    crate::cases::workflow::try_auto_advance(&mut tx, &s, b.case_id).await?;
    tx.commit().await?;
    Ok(Json(json!({"allocated_cents":amount})))
}
#[derive(Deserialize)]
struct Counter {
    case_id: i64,
    invoice_id: i64,
    amount_cents: i64,
    receipt_no: String,
    expected_revision: Option<i64>,
}
async fn counter(State(s): State<AppState>, actor: Actor, Json(b): Json<Counter>) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&s.db).await?;
    manageable(&mut tx, &actor, b.case_id, b.expected_revision).await?;
    let (id, new) = payments::receive(
        &mut tx,
        &actor,
        "counter",
        &b.receipt_no,
        b.amount_cents,
        Some(b.case_id),
        Some(b.invoice_id),
        None,
        None,
    )
    .await?;
    if new {
        confirmed(&mut tx, &s, id, b.case_id).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"payment_id":id})))
}
#[derive(Deserialize)]
struct DepositFilter {
    status: Option<String>,
}
async fn deposit_queue(
    State(s): State<AppState>,
    actor: Actor,
    Query(b): Query<DepositFilter>,
) -> AppResult<Json<Value>> {
    finance(&actor)?;
    if b.status.is_some_and(|s| s != "awaiting_decision") {
        return Err(AppError::field("status", "Use awaiting_decision."));
    }
    let mut c = s.db.acquire().await?;
    Ok(Json(json!(views::queue(&mut c, &actor, "deposits", &time::fmt(s.now())).await?)))
}
#[derive(Deserialize, Serialize)]
struct DepositRequest {
    #[serde(flatten)]
    decision: deposits::Decision,
    expected_revision: Option<i64>,
}
async fn deposit_decision(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    IdempotencyKey(key): IdempotencyKey,
    Json(b): Json<DepositRequest>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&s.db).await?;
    finance(&actor)?;
    authz::require_staff_case(&mut tx, &actor, id).await?;
    let hash = idempotency::json_hash(&serde_json::to_value(&b)?);
    let scope = format!("finance.deposit:{id}");
    if let Some(k) = &key
        && let Some(prev) = idempotency::lookup(&mut tx, actor.user_id, &scope, k, &hash).await?
    {
        return Ok(Json(prev.body));
    }
    manageable(&mut tx, &actor, id, b.expected_revision).await?;
    let decision = deposits::decide(&mut tx, &s, &actor, id, &b.decision).await?;
    crate::cases::workflow::try_auto_advance(&mut tx, &s, id).await?;
    let response = json!({"decision_id":decision});
    if let Some(k) = key {
        idempotency::store(&mut tx, actor.user_id, &scope, &k, &hash, StatusCode::CREATED, &response).await?;
    }
    tx.commit().await?;
    Ok(Json(response))
}
async fn refund_queue(State(s): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    finance(&actor)?;
    let mut c = s.db.acquire().await?;
    Ok(Json(json!(views::queue(&mut c, &actor, "refunds", &time::fmt(s.now())).await?)))
}
#[derive(Deserialize)]
struct Bank {
    bank_reference: String,
    expected_revision: Option<i64>,
}
async fn confirm_bank(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<Bank>,
) -> AppResult<StatusCode> {
    let mut tx = db::write_tx(&s.db).await?;
    let (case, method): (i64, String) =
        sqlx::query_as("SELECT case_id,method FROM refunds WHERE id=?").bind(id).fetch_one(&mut *tx).await?;
    manageable(&mut tx, &actor, case, b.expected_revision).await?;
    if method != "bank_transfer" {
        return Err(AppError::conflict("Provider refunds must be confirmed by DemoPay."));
    }
    if let Some(case) = deposits::complete(&mut tx, &actor, id, Some(&b.bank_reference)).await? {
        crate::cases::workflow::try_auto_advance(&mut tx, &s, case).await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn retry_bank(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<Reason>,
) -> AppResult<StatusCode> {
    let mut tx = db::write_tx(&s.db).await?;
    let case: i64 = sqlx::query_scalar("SELECT case_id FROM refunds WHERE id=? AND status='failed'")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    manageable(&mut tx, &actor, case, b.expected_revision).await?;
    if b.reason.trim().is_empty() {
        return Err(AppError::field("reason", "Explain the recovery action."));
    }
    sqlx::query("UPDATE refunds SET method='bank_transfer',status='processing',failure_reason=NULL WHERE id=?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    ledger::event(
        &mut tx,
        &actor,
        case,
        "finance.refund_recovery",
        &format!("Failed refund will be paid by bank transfer: {}", b.reason),
        json!({"refund_id":id}),
    )
    .await?;
    ledger::tell(
        &mut tx,
        case,
        "Refund being arranged",
        "Finance is arranging your refund by bank transfer. It will be marked completed after confirmation.",
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
struct LedgerFilter {
    case_id: Option<i64>,
}
async fn journal(State(s): State<AppState>, actor: Actor, Query(b): Query<LedgerFilter>) -> AppResult<Json<Value>> {
    finance(&actor)?;
    let mut c = s.db.acquire().await?;
    if let Some(id) = b.case_id {
        authz::require_staff_case(&mut c, &actor, id).await?;
        return Ok(Json(ledger::journal(&mut c, Some(id)).await?));
    }
    let entries=views::scoped(&mut c,&actor,"SELECT e.id,e.at,e.case_id,e.source_type source,e.memo,l.account,l.debit_cents,l.credit_cents FROM journal_entries e JOIN journal_lines l ON l.entry_id=e.id LEFT JOIN cases c ON c.id=e.case_id WHERE c.id IS NULL OR ({scope}) ORDER BY e.id DESC,l.id").await?;
    let mut balances = std::collections::BTreeMap::<String, (i64, i64)>::new();
    for r in &entries {
        let v = balances.entry(r["account"].as_str().unwrap_or_default().into()).or_default();
        v.0 += r["debit_cents"].as_i64().unwrap_or(0);
        v.1 += r["credit_cents"].as_i64().unwrap_or(0);
    }
    Ok(Json(
        json!({"entries":entries,"trial_balance":balances.into_iter().map(|(account,(debit_cents,credit_cents))|json!({"account":account,"debit_cents":debit_cents,"credit_cents":credit_cents,"balance_cents":debit_cents-credit_cents})).collect::<Vec<_>>()}),
    ))
}

async fn unmatch(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<Reason>,
) -> AppResult<StatusCode> {
    let mut tx = db::write_tx(&s.db).await?;
    let case: Option<i64> =
        sqlx::query_scalar("SELECT case_id FROM payments WHERE id=?").bind(id).fetch_one(&mut *tx).await?;
    let case = case.ok_or_else(|| AppError::conflict("Already unmatched."))?;
    manageable(&mut tx, &actor, case, b.expected_revision).await?;
    let cases: Vec<i64> = sqlx::query_scalar("SELECT DISTINCT i.case_id FROM payment_allocations a JOIN invoice_lines l ON l.id=a.invoice_line_id JOIN invoices i ON i.id=l.invoice_id WHERE a.payment_id=? AND a.reversed_at IS NULL").bind(id).fetch_all(&mut *tx).await?;
    for case in cases {
        authz::require_staff_case(&mut tx, &actor, case).await?;
    }
    payments::unmatch(&mut tx, &actor, id, &b.reason).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
struct CreditRefund {
    amount_cents: i64,
    reason: String,
    expected_revision: Option<i64>,
}
async fn refund_credit(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<CreditRefund>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&s.db).await?;
    manageable(&mut tx, &actor, id, b.expected_revision).await?;
    let ids = deposits::refund_credit(&mut tx, &s, &actor, id, b.amount_cents, &b.reason).await?;
    tx.commit().await?;
    Ok(Json(json!({"refund_ids":ids})))
}
#[derive(Deserialize)]
struct Waiver {
    item_code: String,
    amount_cents: i64,
    reason: String,
    expected_revision: i64,
}
async fn waiver(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<Waiver>,
) -> AppResult<StatusCode> {
    let mut tx = db::write_tx(&s.db).await?;
    let (case, _) = authz::require_staff_case(&mut tx, &actor, id).await?;
    if !actor.roles_for_service(case.service_id).contains(&Role::Manager) {
        return Err(AppError::forbidden_msg("A manager must approve a price exemption."));
    }
    if b.amount_cents <= 0 || b.reason.trim().is_empty() {
        return Err(AppError::field("reason", "Enter a positive waiver amount and its reason."));
    }
    let invoices: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM invoices WHERE case_id=? AND kind='invoice')")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if invoices {
        return Err(AppError::conflict("Approve exemptions before invoicing. Use a credit note for issued charges."));
    }
    let (_, lines) = crate::hooks::pricing_lines(&mut tx, &case).await?;
    if !lines.iter().any(|l| l.item_code == b.item_code && l.kind == "fee" && l.amount_cents >= b.amount_cents) {
        return Err(AppError::field("item_code", "Choose a quoted fee and an amount within that fee."));
    }
    core::bump_revision(&mut tx, id, Some(b.expected_revision)).await?;
    sqlx::query("INSERT INTO case_price_waivers(case_id,item_code,amount_cents,reason,approved_by,approved_at) VALUES(?,?,?,?,?,?)").bind(id).bind(&b.item_code).bind(b.amount_cents).bind(&b.reason).bind(actor.user_id).bind(time::fmt(s.now())).execute(&mut *tx).await?;
    ledger::event(
        &mut tx,
        &actor,
        id,
        "finance.waiver_approved",
        &b.reason,
        json!({"item_code":b.item_code,"amount_cents":b.amount_cents,"approved_by":actor.user_id}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::CREATED)
}
