use super::*;
use crate::{
    auth::{Actor, RoleGrant, UserKind},
    authz::Role,
    cases::core,
    db,
    error::ErrorCode,
    state::AppState,
    time,
};
use chrono::{NaiveDate, TimeZone, Utc};
use serde_json::json;
use sqlx::SqliteConnection;
fn date(s: &str) -> NaiveDate {
    time::parse_date(s).unwrap()
}
async fn fixture() -> (AppState, tempfile::TempDir, Actor, i64) {
    let (state, _, dir) =
        crate::state::test_support::test_state_fixed(Utc.with_ymd_and_hms(2026, 10, 7, 0, 0, 0).unwrap()).await;
    let mut tx = db::write_tx(&state.db).await.unwrap();
    sqlx::query("INSERT INTO users(id,email,display_name,kind,created_at) VALUES(1,'fictional@example.test','Fictional applicant','resident','2026-10-01'),(2,'finance@example.test','Fictional finance officer','staff','2026-10-01')").execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO role_grants(user_id,role,granted_at) VALUES(2,'finance','2026-10-01')")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO services(id,slug,name,category,module,department,created_at) VALUES(1,'finance-test','Fictional service','Test','generic','Customer Care','2026-10-01')").execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO service_versions(id,service_id,version,status,definition_json,created_at) VALUES(1,1,1,'published',?,'2026-10-01')").bind(json!({"summary":"Test","outcome":"Test","workflow":{"steps":[{"key":"intake","kind":"review","role":"intake","label":"Check"}]},"pricing":[{"item":"PLANNING_CERT","quantity":1}]}).to_string()).execute(&mut *tx).await.unwrap();
    let c = new_case(&mut tx, "NSH-2026-000001").await;
    seed(&mut tx, &state).await.unwrap();
    seed(&mut tx, &state).await.unwrap();
    balanced(&mut tx).await;
    tx.commit().await.unwrap();
    let actor = Actor {
        user_id: 2,
        kind: UserKind::Staff,
        roles: vec![RoleGrant { role: Role::Finance, scope_service_id: None }],
        display_name: "Fictional finance officer".into(),
        mfa_passed: true,
    };
    (state, dir, actor, c)
}
async fn new_case(tx: &mut SqliteConnection, number: &str) -> i64 {
    sqlx::query_scalar("INSERT INTO cases(number,service_id,service_version_id,module,title,status,applicant_user_id,applicant_name,intake_channel,created_at,submitted_at,updated_at) VALUES(?,1,1,'generic','Fictional case','submitted',1,'Fictional applicant','online','2026-10-01T00:00:00Z','2026-10-01T00:00:00Z','2026-10-01T00:00:00Z') RETURNING id").bind(number).fetch_one(&mut *tx).await.unwrap()
}
/// Assert both the global trial balance and every individual journal entry after every scenario.
async fn balanced(tx: &mut SqliteConnection) {
    let (debit, credit): (i64, i64) =
        sqlx::query_as("SELECT COALESCE(SUM(debit_cents),0),COALESCE(SUM(credit_cents),0) FROM journal_lines")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(debit, credit, "global ledger");
    let bad:i64=sqlx::query_scalar("SELECT COUNT(*) FROM (SELECT entry_id FROM journal_lines GROUP BY entry_id HAVING SUM(debit_cents)<>SUM(credit_cents))").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(bad, 0, "every entry balances");
}
fn line(kind: &str, cents: i64) -> api::QuoteLine {
    api::QuoteLine {
        price_item_id: None,
        price_version_id: None,
        item_code: String::new(),
        kind: kind.into(),
        description: format!("Fictional {kind}"),
        quantity_milli: 1000,
        quantity_minutes: None,
        unit_amount_cents: cents,
        amount_cents: cents,
        calc: json!({}),
    }
}
async fn invoice(tx: &mut SqliteConnection, actor: &Actor, case: i64, lines: &[api::QuoteLine]) -> i64 {
    api::persist_invoice(tx, actor, case, "invoice", date("2026-10-01"), lines, None, None).await.unwrap()
}
async fn inspection(tx: &mut SqliteConnection, case: i64) {
    sqlx::query("INSERT INTO bookable_units(id,code,name,venue) VALUES(1,'fictional-test','Fictional hall','Test') ON CONFLICT DO NOTHING").execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO bookings(case_id,unit_id,status,start_at,end_at,created_at,updated_at) VALUES(?,1,'completed','2026-10-01T01:00:00Z','2026-10-01T03:00:00Z','2026-10-01','2026-10-01')").bind(case).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO tasks(case_id,kind,title,instructions,status,created_at) VALUES(?,'venue_inspection','Fictional hall inspection','Inspect hall','done','2026-10-01')").bind(case).execute(&mut *tx).await.unwrap();
}
#[test]
fn integer_rounding_exact_minutes_and_overflow() {
    assert_eq!(ledger::amount(1, 500, 1000).unwrap(), 1);
    assert_eq!(ledger::amount(1, 499, 1000).unwrap(), 0);
    assert_eq!(ledger::amount(1, 100, 60).unwrap(), 2);
    assert_eq!(ledger::amount(330, 13500, 60).unwrap(), 74250);
    assert!(ledger::amount(i64::MAX, i64::MAX, 1000).is_err());
    assert!(ledger::amount(-1, 100, 1000).is_err());
}
#[tokio::test]
async fn estimates_post_nothing_and_zero_fee_settlement() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    assert!(api::case_settled(&mut tx, c).await.unwrap());
    assert!(api::deposits_settled(&mut tx, c).await.unwrap());
    api::persist_invoice(&mut tx, &a, c, "estimate", date("2026-10-01"), &[line("fee", 1234)], None, None)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(count, 0);
    invoice(&mut tx, &a, c, &[line("fee", 0), line("deposit", 0)]).await;
    assert!(api::case_settled(&mut tx, c).await.unwrap());
    assert!(api::deposits_settled(&mut tx, c).await.unwrap());
    balanced(&mut tx).await;
}
#[tokio::test]
async fn partial_remainder_overpayment_fees_first() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let i = invoice(&mut tx, &a, c, &[line("deposit", 25000), line("fee", 11500)]).await;
    payments::receive(&mut tx, &a, "counter", "partial", 5000, Some(c), Some(i), None, None).await.unwrap();
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT kind,paid_cents FROM finance_line_balances WHERE invoice_id=? ORDER BY kind")
            .bind(i)
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    assert_eq!(rows, vec![("deposit".into(), 0), ("fee".into(), 5000)]);
    balanced(&mut tx).await;
    assert!(!api::case_settled(&mut tx, c).await.unwrap());
    payments::receive(&mut tx, &a, "counter", "remainder", 31500, Some(c), Some(i), None, None).await.unwrap();
    assert!(api::case_settled(&mut tx, c).await.unwrap());
    balanced(&mut tx).await;
    let (p, new) =
        payments::receive(&mut tx, &a, "counter", "excess", 2000, Some(c), Some(i), None, None).await.unwrap();
    assert!(new);
    let credit: i64 = sqlx::query_scalar("SELECT credit_cents FROM finance_payment_balances WHERE payment_id=?")
        .bind(p)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(credit, 2000);
    let summary = api::case_money_summary(&mut tx, c).await.unwrap();
    assert_eq!(summary.outstanding_cents, 0);
    assert_eq!(summary.deposits_held_cents, 25000);
    balanced(&mut tx).await;
    let projection = views::case_money(&mut tx, c, false, &time::fmt(s.now())).await.unwrap();
    assert_eq!(projection["customer_credit_cents"], 2000);
    assert!(projection.get("ledger").is_none());
    assert!(projection.get("allocations").is_none());
}
#[tokio::test]
async fn duplicate_webhook_event_and_payment_ids() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let i = invoice(&mut tx, &a, c, &[line("fee", 11500)]).await;
    sqlx::query("INSERT INTO checkout_sessions(provider_session_id,case_id,invoice_id,amount_cents,status,created_at) VALUES('test-session',?,?,11500,'open','2026-10-01')").bind(c).bind(i).execute(&mut *tx).await.unwrap();
    let mut e = webhooks::Event {
        event_id: "event-1".into(),
        kind: "payment.succeeded".into(),
        session_id: Some("test-session".into()),
        payment_id: Some("payment-1".into()),
        refund_id: None,
        amount_cents: Some(11500),
    };
    assert!(webhooks::process(&mut tx, &e, &serde_json::to_string(&e).unwrap()).await.unwrap().is_some());
    balanced(&mut tx).await;
    assert!(webhooks::process(&mut tx, &e, &serde_json::to_string(&e).unwrap()).await.unwrap().is_none());
    e.event_id = "event-2".into();
    assert!(webhooks::process(&mut tx, &e, &serde_json::to_string(&e).unwrap()).await.unwrap().is_none());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM payments").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(count, 1);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries WHERE source_type='payment'")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(count, 1);
    balanced(&mut tx).await;
}
#[test]
fn signature_and_tolerance() {
    let body = "{\"event_id\":\"x\"}";
    let header = crate::mock::pay::signature("secret", 1000, body);
    assert!(webhooks::verify("secret", &header, body.as_bytes(), 1300));
    assert!(!webhooks::verify("secret", &header, body.as_bytes(), 1301));
    assert!(!webhooks::verify("secret", &header, body.as_bytes(), 699));
    assert!(!webhooks::verify("wrong", &header, body.as_bytes(), 1000));
    assert!(!webhooks::verify("secret", &header, b"tampered", 1000));
    assert!(!webhooks::verify("secret", &format!("{header},t=1000"), body.as_bytes(), 1000));
}
#[tokio::test]
async fn invalid_signature_stores_nothing_and_cannot_poison_money() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let i = invoice(&mut tx, &a, c, &[line("fee", 100)]).await;
    sqlx::query("INSERT INTO checkout_sessions(provider_session_id,case_id,invoice_id,amount_cents,status,created_at) VALUES('s',?,?,100,'open','2026-10-01')").bind(c).bind(i).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let body=json!({"event_id":"legitimate","type":"payment.succeeded","session_id":"s","payment_id":"p","amount_cents":100}).to_string();
    let res = webhooks::handler(
        axum::extract::State(s.clone()),
        axum::http::HeaderMap::new(),
        axum::body::Bytes::from(body.clone()),
    )
    .await
    .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM payments").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(count, 0);
    let invalid: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_events WHERE signature_valid=0")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(invalid, 0);
    let event: webhooks::Event = serde_json::from_str(&body).unwrap();
    assert!(webhooks::process(&mut tx, &event, &body).await.unwrap().is_some());
    balanced(&mut tx).await;
}
#[tokio::test]
async fn statement_file_and_transaction_deduplication() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let i = invoice(&mut tx, &a, c, &[line("fee", 11500)]).await;
    let number: String =
        sqlx::query_scalar("SELECT number FROM invoices WHERE id=?").bind(i).fetch_one(&mut *tx).await.unwrap();
    let csv = format!(
        "date,amount,description,reference,bank_txn_id,payer\n2026-10-07,115.00,Fictional payment,{number},bank-1,Fictional payer\n2026-10-07,10.00,Unknown,unclear,bank-2,Fictional payer\n2026-10-07,115.00,Duplicate,{number},bank-1,Fictional payer\n"
    );
    let (report, confirmed) = statements::import(&mut tx, &s, &a, "demo.csv", &csv).await.unwrap();
    assert_eq!(confirmed.len(), 1);
    assert_eq!(report["rows"][0]["status"], "matched");
    assert_eq!(report["rows"][1]["status"], "unmatched");
    assert_eq!(report["rows"][2]["status"], "duplicate");
    balanced(&mut tx).await;
    tx.commit().await.unwrap();
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let err = statements::import(&mut tx, &s, &a, "renamed.csv", &csv).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    let total: i64 = sqlx::query_scalar("SELECT SUM(amount_cents) FROM payments").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(total, 12500);
    let suspense: i64 = sqlx::query_scalar("SELECT SUM(suspense_cents) FROM finance_payment_balances")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(suspense, 1000);
    let second = csv.replace("Fictional payer", "Different fictional payer");
    let (report, _) = statements::import(&mut tx, &s, &a, "second.csv", &second).await.unwrap();
    assert!(report["rows"].as_array().unwrap().iter().all(|r| r["status"] == "duplicate"));
    balanced(&mut tx).await;
}
#[tokio::test]
async fn ambiguous_reference_and_partial_manual_match() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let i = invoice(&mut tx, &a, c, &[line("fee", 10000)]).await;
    let c2 = new_case(&mut tx, "NSH-2026-000002").await;
    invoice(&mut tx, &a, c2, &[line("fee", 20000)]).await;
    let csv = "date,amount,description,reference,bank_txn_id,payer\n2026-10-07,100.00,Ambiguous,NSH-2026-000001 and NSH-2026-000002,ambiguous,Fictional payer\n2026-10-07,25.00,Partial,NSH-2026-000001,partial,Fictional payer\n";
    let (report, confirmed) = statements::import(&mut tx, &s, &a, "ambiguous.csv", csv).await.unwrap();
    assert!(confirmed.is_empty());
    assert_eq!(report["rows"][0]["status"], "unmatched");
    assert!(report["rows"][0]["suggested_case_id"].is_null());
    assert_eq!(report["rows"][1]["suggested_case_id"], c);
    let p: i64 =
        sqlx::query_scalar("SELECT id FROM payments WHERE external_id='partial'").fetch_one(&mut *tx).await.unwrap();
    payments::match_suspense(&mut tx, &a, p, c, Some(i)).await.unwrap();
    assert_eq!(payments::invoice_due(&mut tx, c, i).await.unwrap(), 7500);
    balanced(&mut tx).await;
    let other: i64 =
        sqlx::query_scalar("SELECT id FROM payments WHERE external_id='ambiguous'").fetch_one(&mut *tx).await.unwrap();
    payments::match_suspense(&mut tx, &a, other, c, Some(i)).await.unwrap();
    assert_eq!(payments::invoice_due(&mut tx, c, i).await.unwrap(), 0);
    let credit: i64 = sqlx::query_scalar("SELECT credit_cents FROM finance_payment_balances WHERE payment_id=?")
        .bind(other)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(credit, 2500);
    balanced(&mut tx).await;
}
#[tokio::test]
async fn reallocation_keeps_history_and_credit_note_settles_old_invoice() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let old = invoice(&mut tx, &a, c, &[line("fee", 10000)]).await;
    let (p, _) = payments::receive(&mut tx, &a, "counter", "one", 10000, Some(c), Some(old), None, None).await.unwrap();
    let allocation: i64 = sqlx::query_scalar("SELECT id FROM payment_allocations WHERE payment_id=?")
        .bind(p)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let line_id: i64 = sqlx::query_scalar("SELECT id FROM invoice_lines WHERE invoice_id=?")
        .bind(old)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    payments::reverse(&mut tx, &a, allocation, "Fictional reschedule").await.unwrap();
    assert_eq!(payments::invoice_due(&mut tx, c, old).await.unwrap(), 10000);
    balanced(&mut tx).await;
    assert!(payments::reverse(&mut tx, &a, allocation, "Repeated reversal").await.is_err());
    let mut credit = line("fee", 10000);
    credit.calc = json!({"original_line_id":line_id});
    api::persist_invoice(
        &mut tx,
        &a,
        c,
        "credit_note",
        date("2026-10-07"),
        &[credit],
        Some("Unused fee credited"),
        Some(old),
    )
    .await
    .unwrap();
    assert!(api::case_settled(&mut tx, c).await.unwrap());
    let revised = invoice(&mut tx, &a, c, &[line("fee", 8000)]).await;
    payments::allocate_credit(&mut tx, &a, p, c, Some(revised)).await.unwrap();
    assert!(api::case_settled(&mut tx, c).await.unwrap());
    let credit: i64 = sqlx::query_scalar("SELECT credit_cents FROM finance_payment_balances WHERE payment_id=?")
        .bind(p)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(credit, 2000);
    let history: Option<String> = sqlx::query_scalar("SELECT reversed_at FROM payment_allocations WHERE id=?")
        .bind(allocation)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(history.is_some());
    balanced(&mut tx).await;
}
#[tokio::test]
async fn old_invoice_keeps_old_rate_and_generic_snapshot_quantity() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let old = api::quote(&mut tx, "HALL_MAIN_DAY", 1000, None, date("2026-10-01")).await.unwrap();
    assert_eq!(old.amount_cents, 11500);
    let i = invoice(&mut tx, &a, c, std::slice::from_ref(&old)).await;
    prices::schedule(&mut tx, &s, &a, "HALL_MAIN_DAY", 13000, "2027-02-01").await.unwrap();
    assert_eq!(api::quote(&mut tx, "HALL_MAIN_DAY", 1000, None, date("2027-01-01")).await.unwrap().amount_cents, 12000);
    assert_eq!(api::quote(&mut tx, "HALL_MAIN_DAY", 1000, None, date("2027-02-01")).await.unwrap().amount_cents, 13000);
    let stored: i64 = sqlx::query_scalar("SELECT unit_amount_cents FROM invoice_lines WHERE invoice_id=?")
        .bind(i)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(stored, 11500);
    assert!(prices::schedule(&mut tx, &s, &a, "RECORD_COPY", 5000, "2026-10-06").await.is_err());
    let case = core::load_case(&mut tx, c).await.unwrap();
    let lines = api::definition_pricing_lines(&mut tx, &case).await.unwrap();
    assert_eq!(lines[0].amount_cents, 18113);
    let qty = api::quote(&mut tx, "EQUIP_EXCAVATOR_HOUR", 17, Some(1), date("2026-10-01")).await.unwrap();
    assert_eq!(qty.amount_cents, 225);
    balanced(&mut tx).await;
}
#[tokio::test]
async fn deposit_settlement_truth_table_and_bank_confirmation() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let i = invoice(&mut tx, &a, c, &[line("deposit", 25000)]).await;
    assert!(!api::deposits_settled(&mut tx, c).await.unwrap());
    assert!(!api::case_settled(&mut tx, c).await.unwrap());
    payments::receive(&mut tx, &a, "bank_transfer", "bond", 25000, Some(c), Some(i), None, None).await.unwrap();
    assert!(api::case_settled(&mut tx, c).await.unwrap());
    assert!(!api::deposits_settled(&mut tx, c).await.unwrap());
    let line_id: i64 = sqlx::query_scalar("SELECT id FROM invoice_lines WHERE invoice_id=?")
        .bind(i)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let decision = deposits::Decision {
        invoice_line_id: line_id,
        refund_cents: 17000,
        retain_items: vec![deposits::RetainItem { label: "Fictional extra cleaning".into(), cents: 8000 }],
        reason: "Fictional inspection: cleaning required".into(),
    };
    assert!(deposits::decide(&mut tx, &s, &a, c, &decision).await.is_err());
    inspection(&mut tx, c).await;
    let d = deposits::decide(&mut tx, &s, &a, c, &decision).await.unwrap();
    assert!(!api::deposits_settled(&mut tx, c).await.unwrap());
    balanced(&mut tx).await;
    let r: i64 = sqlx::query_scalar("SELECT id FROM refunds WHERE deposit_decision_id=?")
        .bind(d)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(deposits::complete(&mut tx, &a, r, Some("")).await.is_err());
    assert!(deposits::complete(&mut tx, &a, r, Some("FICTIONAL-BANK-REF")).await.unwrap().is_some());
    assert!(deposits::complete(&mut tx, &a, r, Some("FICTIONAL-BANK-REF")).await.unwrap().is_none());
    assert!(api::deposits_settled(&mut tx, c).await.unwrap());
    let summary = api::case_money_summary(&mut tx, c).await.unwrap();
    assert_eq!(summary.refunded_cents, 17000);
    assert_eq!(summary.deposits_held_cents, 0);
    balanced(&mut tx).await;
    let allocation: i64 = sqlx::query_scalar("SELECT id FROM payment_allocations WHERE invoice_line_id=?")
        .bind(line_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(payments::reverse(&mut tx, &a, allocation, "Attempt after refund").await.is_err());
}
#[tokio::test]
async fn fully_retained_bond_needs_no_refund() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let i = invoice(&mut tx, &a, c, &[line("deposit", 25000)]).await;
    inspection(&mut tx, c).await;
    payments::receive(&mut tx, &a, "counter", "bond", 25000, Some(c), Some(i), None, None).await.unwrap();
    let l: i64 = sqlx::query_scalar("SELECT id FROM invoice_lines WHERE invoice_id=?")
        .bind(i)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    deposits::decide(
        &mut tx,
        &s,
        &a,
        c,
        &deposits::Decision {
            invoice_line_id: l,
            refund_cents: 0,
            retain_items: vec![deposits::RetainItem { label: "Fictional damage repair".into(), cents: 25000 }],
            reason: "Fictional damage recorded in inspection".into(),
        },
    )
    .await
    .unwrap();
    assert!(api::deposits_settled(&mut tx, c).await.unwrap());
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM refunds").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(n, 0);
    balanced(&mut tx).await;
}
#[tokio::test]
async fn concurrent_refunds_can_never_exceed_paid() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let i = invoice(&mut tx, &a, c, &[line("deposit", 25000)]).await;
    inspection(&mut tx, c).await;
    payments::receive(&mut tx, &a, "bank_transfer", "bond", 25000, Some(c), Some(i), None, None).await.unwrap();
    let l: i64 = sqlx::query_scalar("SELECT id FROM invoice_lines WHERE invoice_id=?")
        .bind(i)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let request = || async {
        let mut tx = db::write_tx(&s.db).await.unwrap();
        let res = deposits::decide(
            &mut tx,
            &s,
            &a,
            c,
            &deposits::Decision {
                invoice_line_id: l,
                refund_cents: 25000,
                retain_items: vec![],
                reason: "Fictional refund request".into(),
            },
        )
        .await;
        if res.is_ok() {
            tx.commit().await.unwrap();
        }
        res
    };
    let (first, second) = tokio::join!(request(), request());
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let reserved: i64 = sqlx::query_scalar("SELECT SUM(amount_cents) FROM refunds").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(reserved, 25000);
    balanced(&mut tx).await;
}
#[tokio::test]
async fn provider_refund_failure_preserves_liability_then_completion() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let i = invoice(&mut tx, &a, c, &[line("deposit", 25000)]).await;
    inspection(&mut tx, c).await;
    payments::receive(&mut tx, &a, "provider", "provider-bond", 25000, Some(c), Some(i), None, None).await.unwrap();
    let l: i64 = sqlx::query_scalar("SELECT id FROM invoice_lines WHERE invoice_id=?")
        .bind(i)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    deposits::decide(
        &mut tx,
        &s,
        &a,
        c,
        &deposits::Decision {
            invoice_line_id: l,
            refund_cents: 25000,
            retain_items: vec![],
            reason: "Fictional full refund".into(),
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE refunds SET provider_refund_id='provider-refund'").execute(&mut *tx).await.unwrap();
    let failed = webhooks::Event {
        event_id: "refund-fail".into(),
        kind: "refund.failed".into(),
        session_id: None,
        payment_id: Some("provider-bond".into()),
        refund_id: Some("provider-refund".into()),
        amount_cents: Some(25000),
    };
    webhooks::process(&mut tx, &failed, &serde_json::to_string(&failed).unwrap()).await.unwrap();
    assert!(!api::deposits_settled(&mut tx, c).await.unwrap());
    let refunded: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries WHERE source_type='refund'")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(refunded, 0);
    balanced(&mut tx).await;
    // Simulate the authorised bank recovery route's update, then use the domain confirmation function.
    sqlx::query("UPDATE refunds SET method='bank_transfer',status='processing'").execute(&mut *tx).await.unwrap();
    let r: i64 = sqlx::query_scalar("SELECT id FROM refunds").fetch_one(&mut *tx).await.unwrap();
    deposits::complete(&mut tx, &a, r, Some("FICTIONAL-RECOVERY")).await.unwrap();
    assert!(api::deposits_settled(&mut tx, c).await.unwrap());
    balanced(&mut tx).await;
}
#[tokio::test]
async fn finance_queues_obey_case_denials_and_confidentiality() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    invoice(&mut tx, &a, c, &[line("fee", 1000)]).await;
    assert_eq!(views::queue(&mut tx, &a, "outstanding", &time::fmt(s.now())).await.unwrap().len(), 1);
    sqlx::query("UPDATE cases SET confidential=1 WHERE id=?").bind(c).execute(&mut *tx).await.unwrap();
    assert!(views::queue(&mut tx, &a, "outstanding", &time::fmt(s.now())).await.unwrap().is_empty());
    sqlx::query("UPDATE cases SET confidential=0 WHERE id=?").bind(c).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO case_access_denials(case_id,user_id,reason,created_at) VALUES(?,2,'Fictional denial','2026-10-07')").bind(c).execute(&mut *tx).await.unwrap();
    assert!(views::queue(&mut tx, &a, "outstanding", &time::fmt(s.now())).await.unwrap().is_empty());
    balanced(&mut tx).await;
}

#[tokio::test]
async fn scheduling_inside_a_future_interval_preserves_later_rate() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    prices::schedule(&mut tx, &s, &a, "HALL_MAIN_DAY", 11800, "2026-12-01").await.unwrap();
    assert_eq!(api::quote(&mut tx, "HALL_MAIN_DAY", 1000, None, date("2026-11-30")).await.unwrap().amount_cents, 11500);
    assert_eq!(api::quote(&mut tx, "HALL_MAIN_DAY", 1000, None, date("2026-12-01")).await.unwrap().amount_cents, 11800);
    assert_eq!(api::quote(&mut tx, "HALL_MAIN_DAY", 1000, None, date("2027-01-01")).await.unwrap().amount_cents, 12000);
    let future = api::quote(&mut tx, "RECORD_COPY", 1000, None, date("2026-12-01")).await.unwrap();
    invoice(&mut tx, &a, c, &[future]).await;
    // This invoice helper prices on 1 Oct; create a second issued invoice with an explicit future date.
    let priced = api::quote(&mut tx, "RECORD_COPY", 1000, None, date("2026-12-01")).await.unwrap();
    api::persist_invoice(&mut tx, &a, c, "invoice", date("2026-12-01"), &[priced], None, None).await.unwrap();
    assert!(prices::schedule(&mut tx, &s, &a, "RECORD_COPY", 5000, "2026-11-01").await.is_err());
    balanced(&mut tx).await;
}

#[tokio::test]
async fn fully_credited_bond_is_settled_and_payment_reentry_does_not_charge_again() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let i = invoice(&mut tx, &a, c, &[line("deposit", 25000)]).await;
    let l: i64 = sqlx::query_scalar("SELECT id FROM invoice_lines WHERE invoice_id=?")
        .bind(i)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let mut credit = line("deposit", 25000);
    credit.calc = json!({"original_line_id":l});
    api::persist_invoice(&mut tx, &a, c, "credit_note", date("2026-10-07"), &[credit], None, Some(i)).await.unwrap();
    assert!(api::case_settled(&mut tx, c).await.unwrap());
    assert!(api::deposits_settled(&mut tx, c).await.unwrap());
    let case = core::load_case(&mut tx, c).await.unwrap();
    assert_eq!(api::ensure_invoice_for_step(&mut tx, &s, &a, &case).await.unwrap(), None);
    balanced(&mut tx).await;
}

/// Exercise the provider's own state, real HTTP HMAC delivery, retry and async refund independently
/// of the records/workflow slices. The receiving handler calls only our money-event domain function.
#[tokio::test]
async fn demopay_redirect_does_not_pay_async_refund_and_delivery_retry() {
    use axum::{
        Router,
        body::Bytes,
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let (mut s, _dir, a, c) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mut cfg = (*s.cfg).clone();
    cfg.public_base_url = base.clone();
    cfg.internal_base_url = base.clone();
    s.cfg = Arc::new(cfg);
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = calls.clone();
    let receiver = s.clone();
    let router = Router::new()
        .merge(crate::mock::pay::routes())
        .route(
            "/api/webhooks/demopay",
            post(move |headers: HeaderMap, body: Bytes| {
                let state = receiver.clone();
                let attempts = callback_calls.clone();
                async move {
                    if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                        return StatusCode::SERVICE_UNAVAILABLE;
                    }
                    assert!(webhooks::verify(
                        &state.cfg.webhook_secret,
                        headers.get("DemoPay-Signature").unwrap().to_str().unwrap(),
                        &body,
                        state.now().timestamp()
                    ));
                    let e: webhooks::Event = serde_json::from_slice(&body).unwrap();
                    let mut tx = db::write_tx(&state.db).await.unwrap();
                    webhooks::process(&mut tx, &e, std::str::from_utf8(&body).unwrap()).await.unwrap();
                    balanced(&mut tx).await;
                    tx.commit().await.unwrap();
                    StatusCode::OK
                }
            }),
        )
        .with_state(s.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let unauthorised=client.post(format!("{base}/mock/pay/api/sessions")).json(&json!({"amount_cents":25000,"currency":"AUD","reference":"Fictional checkout","return_url":format!("{base}/return")})).send().await.unwrap();
    assert_eq!(unauthorised.status(), StatusCode::FORBIDDEN);
    let v:serde_json::Value=client.post(format!("{base}/mock/pay/api/sessions")).header("X-Mock-Key",&s.cfg.mock_api_key).json(&json!({"amount_cents":25000,"currency":"AUD","reference":"Fictional checkout","return_url":format!("{base}/return")})).send().await.unwrap().json().await.unwrap();
    let session = v["session_id"].as_str().unwrap();
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let i = invoice(&mut tx, &a, c, &[line("deposit", 25000)]).await;
    inspection(&mut tx, c).await;
    sqlx::query("INSERT INTO checkout_sessions(provider_session_id,case_id,invoice_id,amount_cents,status,created_at) VALUES(?,?,?,25000,'open','2026-10-07')").bind(session).bind(c).bind(i).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let html = client.get(v["checkout_url"].as_str().unwrap()).send().await.unwrap().text().await.unwrap();
    assert!(html.contains("TEST MODE"));
    assert!(html.contains("Pay with test card"));
    let redirect = client.post(format!("{base}/mock/pay/checkout/{session}/success")).send().await.unwrap();
    assert_eq!(redirect.status(), StatusCode::SEE_OTHER);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM payments").fetch_one(&s.db).await.unwrap();
    assert_eq!(count, 0, "browser redirect never confirms money");
    let event: String = sqlx::query_scalar("SELECT event_id FROM mock_pay_webhook_attempts ORDER BY id LIMIT 1")
        .fetch_one(&s.db)
        .await
        .unwrap();
    let payload = json!({"event_id":event});
    assert!(crate::mock::pay::handle_job(&s, "finance.mock_webhook", &payload).await.is_err());
    crate::mock::pay::handle_job(&s, "finance.mock_webhook", &payload).await.unwrap();
    crate::mock::pay::handle_job(&s, "finance.mock_webhook", &payload).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2, "delivered job replay is a no-op");
    let attempts: i64 = sqlx::query_scalar("SELECT attempts FROM mock_pay_webhook_attempts WHERE event_id=?")
        .bind(&event)
        .fetch_one(&s.db)
        .await
        .unwrap();
    assert_eq!(attempts, 2);
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let l: i64 = sqlx::query_scalar("SELECT id FROM invoice_lines WHERE invoice_id=?")
        .bind(i)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    deposits::decide(
        &mut tx,
        &s,
        &a,
        c,
        &deposits::Decision {
            invoice_line_id: l,
            refund_cents: 25000,
            retain_items: vec![],
            reason: "Fictional successful provider refund".into(),
        },
    )
    .await
    .unwrap();
    let refund: i64 = sqlx::query_scalar("SELECT id FROM refunds").fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    deposits::request_job(&s, refund).await.unwrap();
    deposits::request_job(&s, refund).await.unwrap();
    let (provider, status): (String, String) =
        sqlx::query_as("SELECT provider_refund_id,status FROM refunds WHERE id=?")
            .bind(refund)
            .fetch_one(&s.db)
            .await
            .unwrap();
    assert_eq!(status, "processing");
    let status: String = sqlx::query_scalar("SELECT status FROM mock_pay_refunds WHERE refund_id=?")
        .bind(&provider)
        .fetch_one(&s.db)
        .await
        .unwrap();
    assert_eq!(status, "pending", "PSP returns pending before its async job");
    crate::mock::pay::handle_job(&s, "finance.mock_refund", &json!({"refund_id":provider})).await.unwrap();
    let status: String =
        sqlx::query_scalar("SELECT status FROM refunds WHERE id=?").bind(refund).fetch_one(&s.db).await.unwrap();
    assert_eq!(status, "processing", "provider job alone does not complete our refund");
    let refund_event: String =
        sqlx::query_scalar("SELECT event_id FROM mock_pay_webhook_attempts ORDER BY id DESC LIMIT 1")
            .fetch_one(&s.db)
            .await
            .unwrap();
    crate::mock::pay::handle_job(&s, "finance.mock_webhook", &json!({"event_id":refund_event})).await.unwrap();
    let mut tx = db::write_tx(&s.db).await.unwrap();
    assert!(api::deposits_settled(&mut tx, c).await.unwrap());
    assert_eq!(api::case_money_summary(&mut tx, c).await.unwrap().refunded_cents, 25000);
    balanced(&mut tx).await;
    server.abort();
}

#[tokio::test]
async fn reprice_preserves_valid_bond_allocations_and_surplus_credit() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let i = invoice(&mut tx, &a, c, &[line("fee", 11500), line("deposit", 25000)]).await;
    payments::receive(&mut tx, &a, "counter", "reschedule", 36500, Some(c), Some(i), None, None).await.unwrap();
    let bond_allocation:i64=sqlx::query_scalar("SELECT a.id FROM payment_allocations a JOIN invoice_lines l ON l.id=a.invoice_line_id WHERE l.invoice_id=? AND l.kind='deposit'").bind(i).fetch_one(&mut *tx).await.unwrap();
    let issued = api::reprice_entries(
        &mut tx,
        &s,
        &a,
        c,
        date("2026-11-01"),
        vec![line("fee", 5800), line("deposit", 25000)],
        "Fictional move from main hall to supper room",
    )
    .await
    .unwrap();
    assert_eq!(issued.len(), 2);
    let reversed: Option<String> = sqlx::query_scalar("SELECT reversed_at FROM payment_allocations WHERE id=?")
        .bind(bond_allocation)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(reversed.is_none());
    assert!(api::case_settled(&mut tx, c).await.unwrap());
    let credit: i64 =
        sqlx::query_scalar("SELECT SUM(credit_cents) FROM finance_payment_balances").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(credit, 5700);
    let summary = api::case_money_summary(&mut tx, c).await.unwrap();
    assert_eq!(summary.deposits_held_cents, 25000);
    balanced(&mut tx).await;
}

#[tokio::test]
async fn credit_can_move_to_same_applicant_but_not_another_customer() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let old = invoice(&mut tx, &a, c, &[line("fee", 1000)]).await;
    let (p, _) =
        payments::receive(&mut tx, &a, "counter", "overpaid", 2000, Some(c), Some(old), None, None).await.unwrap();
    let other = new_case(&mut tx, "NSH-2026-000002").await;
    let target = invoice(&mut tx, &a, other, &[line("fee", 750)]).await;
    assert_eq!(payments::allocate_credit(&mut tx, &a, p, other, Some(target)).await.unwrap(), 750);
    assert!(api::case_settled(&mut tx, other).await.unwrap());
    balanced(&mut tx).await;
    let stranger = new_case(&mut tx, "NSH-2026-000003").await;
    sqlx::query("UPDATE cases SET applicant_user_id=2 WHERE id=?").bind(stranger).execute(&mut *tx).await.unwrap();
    let wrong = invoice(&mut tx, &a, stranger, &[line("fee", 250)]).await;
    assert!(payments::allocate_credit(&mut tx, &a, p, stranger, Some(wrong)).await.is_err());
    balanced(&mut tx).await;
}

#[tokio::test]
async fn late_provider_receipt_after_counter_settlement_becomes_credit() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let i = invoice(&mut tx, &a, c, &[line("fee", 1000)]).await;
    sqlx::query("INSERT INTO checkout_sessions(provider_session_id,case_id,invoice_id,amount_cents,status,created_at) VALUES('late-checkout',?,?,1000,'open','2026-10-07')").bind(c).bind(i).execute(&mut *tx).await.unwrap();
    payments::receive(&mut tx, &a, "counter", "counter-first", 1000, Some(c), Some(i), None, None).await.unwrap();
    let status: String =
        sqlx::query_scalar("SELECT status FROM checkout_sessions WHERE provider_session_id='late-checkout'")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(status, "expired");
    let e = webhooks::Event {
        event_id: "late-event".into(),
        kind: "payment.succeeded".into(),
        session_id: Some("late-checkout".into()),
        payment_id: Some("late-payment".into()),
        refund_id: None,
        amount_cents: Some(1000),
    };
    assert!(webhooks::process(&mut tx, &e, &serde_json::to_string(&e).unwrap()).await.unwrap().is_some());
    let credit: i64 =
        sqlx::query_scalar("SELECT SUM(credit_cents) FROM finance_payment_balances").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(credit, 1000);
    assert!(api::case_settled(&mut tx, c).await.unwrap());
    balanced(&mut tx).await;
}

#[tokio::test]
async fn cancelled_unused_paid_booking_bond_refunds_without_inspection() {
    let (s, _dir, a, c) = fixture().await;
    let mut tx = db::write_tx(&s.db).await.unwrap();
    let i = invoice(&mut tx, &a, c, &[line("deposit", 25000)]).await;
    payments::receive(&mut tx, &a, "bank_transfer", "cancelled-bond", 25000, Some(c), Some(i), None, None)
        .await
        .unwrap();
    sqlx::query("INSERT INTO bookable_units(id,code,name,venue) VALUES(1,'test','Test hall','Test')")
        .execute(&mut *tx)
        .await
        .unwrap();
    let booking:i64=sqlx::query_scalar("INSERT INTO bookings(case_id,unit_id,status,start_at,end_at,created_at,updated_at) VALUES(?,1,'cancelled','2030-10-01T01:00:00Z','2030-10-01T03:00:00Z','2026-10-01','2026-10-01') RETURNING id").bind(c).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO booking_cancellations(booking_id,cancelled_at,unused,reason) VALUES(?,'2026-10-07',1,'Unused hire cancelled')").bind(booking).execute(&mut *tx).await.unwrap();
    assert!(deposits::ready(&mut tx, c, &time::fmt(s.now())).await.unwrap());
    assert_eq!(views::queue(&mut tx, &a, "deposits", &time::fmt(s.now())).await.unwrap().len(), 1);
    let lid = sqlx::query_scalar("SELECT id FROM invoice_lines WHERE invoice_id=?")
        .bind(i)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let decision = deposits::Decision {
        invoice_line_id: lid,
        refund_cents: 25000,
        retain_items: vec![],
        reason: "Full unused bond return".into(),
    };
    let d = deposits::decide(&mut tx, &s, &a, c, &decision).await.unwrap();
    assert!(deposits::decide(&mut tx, &s, &a, c, &decision).await.is_err());
    assert!(!api::deposits_settled(&mut tx, c).await.unwrap());
    let r = sqlx::query_scalar("SELECT id FROM refunds WHERE deposit_decision_id=?")
        .bind(d)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    deposits::complete(&mut tx, &a, r, Some("Cancellation return")).await.unwrap();
    assert!(api::deposits_settled(&mut tx, c).await.unwrap());
    balanced(&mut tx).await;
}

#[tokio::test]
async fn webhook_rejections_are_size_rate_and_concurrency_limited_without_storage() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let (s, _dir) = crate::state::test_support::test_state().await;
    let app = crate::app::build_router(s.clone());
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/webhooks/demopay")
                .body(Body::from(vec![b'x'; webhooks::MAX_WEBHOOK_BYTES + 1]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let mut limited = false;
    for _ in 0..35 {
        let response = app
            .clone()
            .oneshot(Request::post("/api/webhooks/demopay").body(Body::from("unsigned")).unwrap())
            .await
            .unwrap();
        limited |= response.status() == StatusCode::TOO_MANY_REQUESTS;
    }
    assert!(limited);
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM provider_events").fetch_one(&s.db).await.unwrap(), 0);
}
