use super::payments;
use crate::{
    auth::Actor,
    error::{AppError, AppResult},
    state::AppState,
    storage::{self, AllowList},
    time,
};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
/// Exact positive decimal parser for money and definition quantities; no floating point.
pub fn decimal_units(s: &str, places: u32) -> AppResult<i64> {
    let s = s.trim();
    let parts: Vec<_> = s.split('.').collect();
    if parts.len() > 2
        || parts[0].is_empty()
        || !parts[0].bytes().all(|c| c.is_ascii_digit())
        || parts.get(1).is_some_and(|p| p.len() > places as usize || !p.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err(AppError::field("amount", "Use a positive decimal amount without currency symbols."));
    }
    let scale = 10i64.pow(places);
    let whole = parts[0].parse::<i64>().map_err(|_| AppError::field("amount", "Amount is too large."))?;
    let fraction =
        parts.get(1).map(|p| format!("{p:0<width$}", width = places as usize).parse::<i64>().unwrap_or(0)).unwrap_or(0);
    whole
        .checked_mul(scale)
        .and_then(|v| v.checked_add(fraction))
        .ok_or_else(|| AppError::field("amount", "Amount is too large."))
}
fn csv(text: &str) -> AppResult<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !closed => quoted = true,
            ',' => {
                row.push(std::mem::take(&mut field));
                closed = false;
            }
            '\n' => {
                row.push(std::mem::take(&mut field));
                if row.iter().any(|f| !f.trim().is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
                closed = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            _ if closed => return Err(AppError::field("file", "Unexpected character after a quoted CSV field.")),
            '"' => return Err(AppError::field("file", "Quote CSV fields containing commas.")),
            _ => field.push(c),
        }
    }
    if quoted {
        return Err(AppError::field("file", "The CSV has an unclosed quote."));
    }
    row.push(field);
    if row.iter().any(|f| !f.trim().is_empty()) {
        rows.push(row);
    }
    Ok(rows)
}
#[derive(Debug)]
pub struct Transfer {
    pub date: String,
    pub cents: i64,
    pub description: String,
    pub reference: String,
    pub external: String,
    pub payer: String,
}
pub fn parse(text: &str) -> AppResult<Vec<Transfer>> {
    let rows = csv(text)?;
    let expected = ["date", "amount", "description", "reference", "bank_txn_id", "payer"];
    if rows.first().map(|r| r.iter().map(|s| s.trim()).collect::<Vec<_>>()) != Some(expected.to_vec()) {
        return Err(AppError::field("file", "CSV header must be date,amount,description,reference,bank_txn_id,payer."));
    }
    if rows.len() > 10001 {
        return Err(AppError::field("file", "Import at most 10,000 rows."));
    }
    rows.into_iter()
        .skip(1)
        .enumerate()
        .map(|(n, r)| {
            if r.len() != 6 {
                return Err(AppError::field("file", format!("Row {} must contain six fields.", n + 2)));
            }
            time::parse_date(r[0].trim())
                .map_err(|_| AppError::field("file", format!("Row {} has an invalid date.", n + 2)))?;
            let cents = decimal_units(&r[1], 2)?;
            if cents == 0 || r[4].trim().is_empty() {
                return Err(AppError::field("file", "Every row needs a positive amount and a bank transaction ID."));
            }
            Ok(Transfer {
                date: r[0].trim().into(),
                cents,
                description: r[2].clone(),
                reference: r[3].clone(),
                external: r[4].trim().into(),
                payer: r[5].clone(),
            })
        })
        .collect()
}
/// Contains a whole identifier, not a prefix of another invoice/case number.
fn mentions(reference: &str, number: &str) -> bool {
    let reference = reference.to_ascii_uppercase();
    let number = number.to_ascii_uppercase();
    reference.match_indices(&number).any(|(i, _)| {
        let before = reference[..i].chars().next_back();
        let after = reference[i + number.len()..].chars().next();
        before.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '-')
            && after.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '-')
    })
}
async fn candidate(tx: &mut SqliteConnection, actor: &Actor, row: &Transfer) -> AppResult<(Option<i64>, Option<i64>)> {
    let scope = crate::authz::case_scope_sql(actor);
    let sql = format!(
        "SELECT c.id,c.number,i.id,i.number FROM cases c LEFT JOIN invoices i ON i.case_id=c.id AND i.kind='invoice' AND i.status='issued' WHERE {}",
        scope.sql
    );
    type Candidate = (i64, Option<String>, Option<i64>, Option<String>);
    let rows: Vec<Candidate> = crate::db::bind_all_as(sqlx::query_as(&sql), &scope.binds).fetch_all(&mut *tx).await?;
    let mut cases = std::collections::BTreeSet::new();
    let mut exact = std::collections::BTreeSet::new();
    for (case, case_number, invoice, invoice_number) in &rows {
        if invoice_number.as_ref().is_some_and(|n| mentions(&row.reference, n))
            || case_number.as_ref().is_some_and(|n| mentions(&row.reference, n))
        {
            cases.insert(*case);
            if let Some(i) = invoice
                && payments::invoice_due(tx, *case, *i).await? == row.cents
            {
                exact.insert((*case, *i));
            }
        }
    }
    if cases.len() == 1 {
        let c = *cases.first().unwrap();
        return Ok((Some(c), if exact.len() == 1 { exact.first().map(|(_, i)| *i) } else { None }));
    }
    if !cases.is_empty() {
        return Ok((None, None));
    }
    // An amount/payer hint is a suggestion only, never an automatic match.
    for (case, _, invoice, _) in rows {
        if let Some(i) = invoice
            && payments::invoice_due(tx, case, i).await? == row.cents
        {
            cases.insert(case);
        }
    }
    Ok((if cases.len() == 1 { cases.first().copied() } else { None }, None))
}
pub async fn import(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    name: &str,
    text: &str,
) -> AppResult<(Value, Vec<(i64, i64)>)> {
    let transfers = parse(text)?;
    let hash = crate::idempotency::request_hash(text.as_bytes());
    let duplicate: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM statement_imports WHERE file_sha256=?)")
        .bind(&hash)
        .fetch_one(&mut *tx)
        .await?;
    if duplicate {
        return Err(AppError::conflict("This statement file has already been imported."));
    }
    let staged = storage::stage(state, text.as_bytes(), name, AllowList::Data).await?;
    let blob = storage::register(tx, staged, actor.db_id()).await?;
    let id:i64=sqlx::query_scalar("INSERT INTO statement_imports(filename,blob_id,file_sha256,imported_by,imported_at,row_count) VALUES(?,?,?,?,?,?) RETURNING id").bind(name).bind(blob.id).bind(hash).bind(actor.db_id()).bind(time::fmt(state.now())).bind(transfers.len() as i64).fetch_one(&mut *tx).await?;
    let mut report = Vec::new();
    let mut confirmed = Vec::new();
    for row in transfers {
        let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM statement_rows WHERE bank_txn_id=? AND status<>'duplicate') OR EXISTS(SELECT 1 FROM payments WHERE source='bank_transfer' AND external_id=?)").bind(&row.external).bind(&row.external).fetch_one(&mut *tx).await?;
        let (case, invoice) = if exists { (None, None) } else { candidate(tx, actor, &row).await? };
        let status = if exists {
            "duplicate"
        } else if invoice.is_some() {
            "matched"
        } else {
            "unmatched"
        };
        let payment = if !exists {
            let (p, new) = payments::receive(
                tx,
                actor,
                "bank_transfer",
                &row.external,
                row.cents,
                if invoice.is_some() { case } else { None },
                invoice,
                Some(&row.payer),
                Some(&row.reference),
            )
            .await?;
            let received = time::local_to_utc(time::parse_date(&row.date)?, chrono::NaiveTime::MIN);
            sqlx::query("UPDATE payments SET received_at=? WHERE id=?")
                .bind(time::fmt(received))
                .bind(p)
                .execute(&mut *tx)
                .await?;
            if let Some(c) = case.filter(|_| invoice.is_some() && new) {
                confirmed.push((p, c));
            }
            Some(p)
        } else {
            None
        };
        let row_id:i64=sqlx::query_scalar("INSERT INTO statement_rows(import_id,bank_txn_id,txn_date,amount_cents,payer_name,description,reference,status,suggested_case_id,payment_id) VALUES(?,?,?,?,?,?,?,?,?,?) RETURNING id").bind(id).bind(&row.external).bind(&row.date).bind(row.cents).bind(&row.payer).bind(&row.description).bind(&row.reference).bind(status).bind(case).bind(payment).fetch_one(&mut *tx).await?;
        report.push(json!({"id":row_id,"bank_txn_id":row.external,"status":status,"amount_cents":row.cents,"suggested_case_id":case}));
    }
    crate::audit::record(
        tx,
        actor.db_id(),
        "finance.statement_imported",
        "statement_import",
        Some(id),
        json!({"rows":report.len()}),
    )
    .await?;
    Ok((json!({"import_id":id,"rows":report}), confirmed))
}
pub async fn ignore(tx: &mut SqliteConnection, actor: &Actor, id: i64, note: &str) -> AppResult<()> {
    if note.trim().is_empty() {
        return Err(AppError::field("note", "Explain why this row is ignored."));
    }
    let updated=sqlx::query("UPDATE statement_rows SET status='ignored',resolved_by=?,resolved_at=?,note=? WHERE id=? AND status='unmatched'").bind(actor.db_id()).bind(time::now_str()).bind(note).bind(id).execute(&mut *tx).await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::conflict("Only an unmatched row can be ignored."));
    }
    // The money remains in unallocated_receipts. Ignoring evidence is not a write-off.
    crate::audit::record(
        tx,
        actor.db_id(),
        "finance.statement_ignored",
        "statement_row",
        Some(id),
        json!({"note":note}),
    )
    .await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_decimal_and_quoted_csv() {
        assert_eq!(decimal_units("181.13", 2).unwrap(), 18113);
        assert_eq!(decimal_units("0.001", 3).unwrap(), 1);
        assert!(decimal_units("1.001", 2).is_err());
        assert!(decimal_units("-2", 2).is_err());
        let r=parse("date,amount,description,reference,bank_txn_id,payer\n2026-10-07,2.50,\"a,b\",INV-2026-00001,x,\"Fictional, Person\"\n").unwrap();
        assert_eq!(r[0].cents, 250);
        assert_eq!(r[0].description, "a,b");
        assert!(!mentions("INV-2026-000012", "INV-2026-00001"));
    }
}
