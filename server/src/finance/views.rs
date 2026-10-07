use super::{api, deposits, ledger};
use crate::{auth::Actor, error::AppResult, time};
use serde_json::{Map, Value, json};
use sqlx::{Column, Row, SqliteConnection, TypeInfo, ValueRef};
/// JSON rows for finance projections only. SQL stays parameterised at the call site.
pub fn row(r: &sqlx::sqlite::SqliteRow) -> Value {
    let mut value = Map::new();
    for c in r.columns() {
        let name = c.name();
        let raw = r.try_get_raw(name).expect("selected column");
        let v = if raw.is_null() {
            Value::Null
        } else {
            match raw.type_info().name() {
                "INTEGER" | "BOOLEAN" => json!(r.get::<i64, _>(name)),
                "REAL" => json!(r.get::<f64, _>(name)),
                _ => json!(r.get::<String, _>(name)),
            }
        };
        value.insert(name.into(), v);
    }
    Value::Object(value)
}
pub async fn case_money(tx: &mut SqliteConnection, case: i64, staff: bool, now: &str) -> AppResult<Value> {
    let summary = api::case_money_summary(tx, case).await?;
    let invoices=sqlx::query("SELECT i.*,d.document_id,d.document_version_id FROM invoices i LEFT JOIN finance_invoice_documents d ON d.invoice_id=i.id WHERE i.case_id=? ORDER BY i.id DESC").bind(case).fetch_all(&mut *tx).await?;
    let mut issued = Vec::new();
    for i in invoices {
        let id: i64 = i.get("id");
        let mut v = row(&i);
        let lines=sqlx::query("SELECT *,MAX(0,amount_cents-credited_cents-paid_cents) outstanding_cents FROM finance_line_balances WHERE invoice_id=? ORDER BY id").bind(id).fetch_all(&mut *tx).await?;
        v["lines"] = json!(lines.iter().map(row).collect::<Vec<_>>());
        v["outstanding_cents"] = json!(if i.get::<String, _>("kind") == "invoice" {
            lines.iter().map(|l| l.get::<i64, _>("outstanding_cents")).sum::<i64>()
        } else {
            0
        });
        issued.push(v);
    }
    let payments=sqlx::query("SELECT p.id,p.source,p.amount_cents,p.received_at,p.status,b.credit_cents FROM payments p JOIN finance_payment_balances b ON b.payment_id=p.id WHERE p.case_id=? OR EXISTS(SELECT 1 FROM payment_allocations a JOIN invoice_lines l ON l.id=a.invoice_line_id JOIN invoices i ON i.id=l.invoice_id WHERE a.payment_id=p.id AND i.case_id=? AND a.reversed_at IS NULL) ORDER BY p.id DESC").bind(case).bind(case).fetch_all(&mut *tx).await?;
    let credit:i64=sqlx::query_scalar("SELECT COALESCE(SUM(b.credit_cents),0) FROM finance_payment_balances b JOIN payments p ON p.id=b.payment_id WHERE p.case_id=? AND p.status='confirmed'").bind(case).fetch_one(&mut *tx).await?;
    let decisions = sqlx::query("SELECT * FROM deposit_decisions WHERE case_id=? ORDER BY id")
        .bind(case)
        .fetch_all(&mut *tx)
        .await?;
    let decisions: Vec<Value> = decisions
        .iter()
        .map(|r| {
            let mut v = row(r);
            v["retain_items"] = serde_json::from_str(&r.get::<String, _>("calc_json")).unwrap_or(json!([]));
            v
        })
        .collect();
    let refunds=sqlx::query("SELECT id,payment_id,deposit_decision_id,amount_cents,method,status,reason,created_at,completed_at,failure_reason,bank_reference FROM refunds WHERE case_id=? ORDER BY id DESC").bind(case).fetch_all(&mut *tx).await?;
    let sessions=sqlx::query("SELECT id,invoice_id,amount_cents,status,created_at,completed_at FROM checkout_sessions WHERE case_id=? ORDER BY id DESC").bind(case).fetch_all(&mut *tx).await?;
    let revision: i64 =
        sqlx::query_scalar("SELECT revision FROM cases WHERE id=?").bind(case).fetch_one(&mut *tx).await?;
    let mut result = json!({"summary":summary,"invoices":issued,"payments":payments.iter().map(row).collect::<Vec<_>>(),"deposit_decisions":decisions,"refunds":refunds.iter().map(row).collect::<Vec<_>>(),"checkout_sessions":sessions.iter().map(row).collect::<Vec<_>>(),"customer_credit_cents":credit,"staff":staff,"revision":revision,"deposit_ready":deposits::ready(tx,case,now).await?,"schedule_note":"FY2026-27 schedule (demo copy — confirm with Council)"});
    if staff {
        let allocations=sqlx::query("SELECT a.*,p.source,p.external_id FROM payment_allocations a JOIN payments p ON p.id=a.payment_id JOIN invoice_lines l ON l.id=a.invoice_line_id JOIN invoices i ON i.id=l.invoice_id WHERE i.case_id=? ORDER BY a.id DESC").bind(case).fetch_all(&mut *tx).await?;
        let evidence=sqlx::query("SELECT d.id,d.title,d.created_at,(SELECT v.id FROM document_versions v WHERE v.document_id=d.id ORDER BY v.version DESC LIMIT 1) document_version_id FROM documents d WHERE d.case_id=? AND d.category='receipt' AND d.disposed_at IS NULL ORDER BY d.id DESC").bind(case).fetch_all(&mut *tx).await?;
        result["allocations"] = json!(allocations.iter().map(row).collect::<Vec<_>>());
        result["evidence"] = json!(evidence.iter().map(row).collect::<Vec<_>>());
        result["ledger"] = ledger::journal(tx, Some(case)).await?;
    }
    Ok(result)
}
pub async fn scoped(tx: &mut SqliteConnection, actor: &Actor, sql: &str) -> AppResult<Vec<Value>> {
    let scope = crate::authz::case_scope_sql(actor);
    let query = sql.replace("{scope}", &scope.sql);
    let rows = crate::db::bind_all(sqlx::query(&query), &scope.binds).fetch_all(&mut *tx).await?;
    Ok(rows.iter().map(row).collect())
}
pub async fn queue(tx: &mut SqliteConnection, actor: &Actor, kind: &str, now: &str) -> AppResult<Vec<Value>> {
    let rows=match kind{
        "unmatched"=>scoped(tx,actor,"SELECT s.* FROM statement_rows s LEFT JOIN cases c ON c.id=s.suggested_case_id WHERE s.status='unmatched' AND (c.id IS NULL OR ({scope})) ORDER BY s.id DESC").await?,
        "refunds"=>scoped(tx,actor,"SELECT r.*,c.number case_number,c.applicant_name,c.revision case_revision FROM refunds r JOIN cases c ON c.id=r.case_id WHERE {scope} ORDER BY r.id DESC").await?,
        "deposits" => {
            let candidates = scoped(tx,actor,"SELECT l.id invoice_line_id,l.description,l.amount_cents,l.paid_cents,i.case_id,c.number case_number,c.applicant_name,c.revision case_revision,b.end_at FROM finance_line_balances l JOIN invoices i ON i.id=l.invoice_id JOIN cases c ON c.id=i.case_id JOIN bookings b ON b.case_id=c.id WHERE {scope} AND i.kind='invoice' AND i.status='issued' AND l.kind='deposit' AND l.paid_cents>0 AND l.paid_cents=l.amount_cents-l.credited_cents AND NOT EXISTS(SELECT 1 FROM deposit_decisions d WHERE d.invoice_line_id=l.id) ORDER BY b.end_at").await?;
            let mut ready = Vec::new();
            for row in candidates { if deposits::ready(tx,row["case_id"].as_i64().expect("selected case"),now).await? { ready.push(row); } }
            ready
        },
        "outstanding"=>scoped(tx,actor,"SELECT i.id,i.case_id,i.number,c.number case_number,c.applicant_name,SUM(MAX(0,l.amount_cents-l.credited_cents-l.paid_cents)) outstanding_cents FROM invoices i JOIN finance_line_balances l ON l.invoice_id=i.id JOIN cases c ON c.id=i.case_id WHERE {scope} AND i.kind='invoice' AND i.status='issued' GROUP BY i.id HAVING outstanding_cents>0 ORDER BY i.id").await?,
        "receipts"=>scoped(tx,actor,"SELECT p.*,c.number case_number FROM payments p LEFT JOIN cases c ON c.id=p.case_id WHERE p.status='confirmed' AND (c.id IS NULL OR ({scope})) ORDER BY p.id DESC").await?,
        _=>Vec::new(),
    };
    Ok(rows)
}
pub async fn overview(tx: &mut SqliteConnection, actor: &Actor, now: &str) -> AppResult<Value> {
    let receipts = queue(tx, actor, "receipts", now).await?;
    let today = time::local_date(time::parse(now)?);
    let today: Vec<_> = receipts
        .into_iter()
        .filter(|r| {
            r["received_at"].as_str().and_then(|s| time::parse(s).ok()).is_some_and(|d| time::local_date(d) == today)
        })
        .collect();
    let refunds: Vec<_> = queue(tx, actor, "refunds", now)
        .await?
        .into_iter()
        .filter(|r| matches!(r["status"].as_str(), Some("processing" | "requested" | "failed")))
        .collect();
    Ok(
        json!({"unmatched":queue(tx,actor,"unmatched",now).await?,"refunds":refunds,"deposits":queue(tx,actor,"deposits",now).await?,"outstanding_invoices":queue(tx,actor,"outstanding",now).await?,"todays_receipts":today}),
    )
}
