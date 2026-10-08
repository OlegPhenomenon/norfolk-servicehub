//! Building fee assessment (audit N-01). A building route with a `finance.fee_assessed` step is invoiced only
//! from a recorded assessment: a schedule calculation (stored scale or price item, with its inputs and basis)
//! or a staff assessment with amount and reason. Re-assessment after invoicing never edits the issued
//! invoice: an increase is billed on a supplementary invoice, a decrease is returned by a credit note (any
//! money already paid becomes customer credit that Finance can refund). Exemptions stay the explicit manager
//! waiver (`case_price_waivers`) approved before invoicing, which leaves a zero-value invoice line on record.
use super::api::{self, QuoteLine};
use super::{ledger, payments};
use crate::{
    auth::Actor,
    authz::{self, CaseAccess, Role},
    cases::core::{self, CaseRow, Visibility},
    db,
    documents::building::{self, RouteKind},
    error::{AppError, AppResult},
    state::AppState,
    time,
    web::{Json, Path},
};
use axum::extract::State;
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub const HANDLER: &str = "finance.fee_assessed";
const SCALE: &str = "BUILDING_WORKS";
const SCALE_ITEM: &str = "BUILDING_WORKS_FEE";
const BASIC_ITEM: &str = "MODIFICATION_FEE";
const NOTE: &str = "FY2026-27 schedule (demo copy — confirm with Council)";
const MODIFICATION_TYPES: [&str; 4] = ["minor_error", "conditions", "lapse_date", "other"];

/// Demo copy of the FY2026-27 "Building Development and Works" scale: (over, up to, base, cents per $1,000).
pub async fn seed_scale(tx: &mut SqliteConnection) -> AppResult<()> {
    let bands: [(i64, Option<i64>, i64, i64); 6] = [
        (0, Some(5_000_000), 57_000, 0),
        (5_000_000, Some(25_000_000), 60_000, 400),
        (25_000_000, Some(50_000_000), 160_000, 257),
        (50_000_000, Some(100_000_000), 230_000, 164),
        (100_000_000, Some(1_000_000_000), 400_000, 177),
        (1_000_000_000, None, 2_500_000, 131),
    ];
    for (over, up_to, base, rate) in bands {
        sqlx::query("INSERT INTO fee_scale_bands(scale_code,effective_from,over_cents,up_to_cents,base_cents,rate_cents_per_1000,source_note) VALUES(?,'2026-07-01',?,?,?,?,?) ON CONFLICT DO NOTHING")
            .bind(SCALE).bind(over).bind(up_to).bind(base).bind(rate)
            .bind("NIRC Fees and Charges FY2026-27, Building Development and Works (pp.16–18). Demo copy — confirm with Council.")
            .execute(&mut *tx).await?;
    }
    Ok(())
}

pub async fn scale_rows(tx: &mut SqliteConnection) -> AppResult<Value> {
    let rows = sqlx::query("SELECT * FROM fee_scale_bands ORDER BY scale_code,effective_from,over_cents")
        .fetch_all(&mut *tx)
        .await?;
    Ok(json!(rows.iter().map(|r| json!({"id":r.get::<i64,_>("id"),"scale_code":r.get::<String,_>("scale_code"),"effective_from":r.get::<String,_>("effective_from"),"effective_to":r.get::<Option<String>,_>("effective_to"),"over_cents":r.get::<i64,_>("over_cents"),"up_to_cents":r.get::<Option<i64>,_>("up_to_cents"),"base_cents":r.get::<i64,_>("base_cents"),"rate_cents_per_1000":r.get::<i64,_>("rate_cents_per_1000"),"source_note":r.get::<String,_>("source_note")})).collect::<Vec<_>>()))
}

/// The case's frozen workflow contains the fee-assessment checkpoint.
pub async fn applies(tx: &mut SqliteConnection, case: &CaseRow) -> AppResult<bool> {
    let def = crate::services::definition::load_for_case(tx, case).await?;
    Ok(def.workflow.steps.iter().any(|s| s.handler.as_deref() == Some(HANDLER)))
}

#[derive(Debug, Clone)]
pub struct Assessment {
    pub id: i64,
    pub version: i64,
    pub amount_cents: i64,
    pub lines: Vec<QuoteLine>,
    pub charged_cents: Option<i64>,
}
pub async fn current(tx: &mut SqliteConnection, case: i64) -> AppResult<Option<Assessment>> {
    let row = sqlx::query("SELECT id,version,amount_cents,lines_json,charged_cents FROM building_fee_assessments WHERE case_id=? ORDER BY version DESC LIMIT 1")
        .bind(case)
        .fetch_optional(&mut *tx)
        .await?;
    row.map(|r| {
        Ok(Assessment {
            id: r.get("id"),
            version: r.get("version"),
            amount_cents: r.get("amount_cents"),
            lines: serde_json::from_str(&r.get::<String, _>("lines_json"))?,
            charged_cents: r.get("charged_cents"),
        })
    })
    .transpose()
}

fn pricing_date(case: &CaseRow) -> AppResult<NaiveDate> {
    Ok(time::local_date(time::parse(case.submitted_at.as_deref().unwrap_or(&case.created_at))?))
}
async fn answers(tx: &mut SqliteConnection, case: i64) -> AppResult<Value> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT answers_json FROM submissions WHERE case_id=? ORDER BY id DESC LIMIT 1")
            .bind(case)
            .fetch_optional(&mut *tx)
            .await?;
    Ok(raw.map(|s| serde_json::from_str(&s)).transpose()?.unwrap_or_else(|| json!({})))
}
fn cost_cents(v: &Value) -> AppResult<Option<i64>> {
    let text = match v {
        Value::Null => return Ok(None),
        Value::Number(n) => n.to_string(),
        Value::String(s) if s.trim().is_empty() => return Ok(None),
        Value::String(s) => s.replace([',', '$'], ""),
        _ => return Err(AppError::field("estimated_cost", "Enter the estimated cost in dollars.")),
    };
    super::statements::decimal_units(&text, 2)
        .map(Some)
        .map_err(|_| AppError::field("estimated_cost", "Enter the estimated cost as a positive amount in dollars."))
}
/// Modification types from the B-slice multiselect, or the legacy single select of older snapshots.
fn answer_types(a: &Value) -> Vec<String> {
    if let Some(list) = a.get("modification_types").and_then(Value::as_array) {
        return list.iter().filter_map(Value::as_str).map(String::from).collect();
    }
    match a.get("modification_type").and_then(Value::as_str) {
        Some("Lapse date") => vec!["lapse_date".into()],
        Some("Minor error") => vec!["minor_error".into()],
        Some("Conditions") => vec!["conditions".into()],
        Some("Other") => vec!["other".into()],
        _ => vec![],
    }
}

struct Proposal {
    rule: &'static str,
    lines: Vec<QuoteLine>,
    amount: i64,
    explanation: String,
}
async fn scale_line(
    tx: &mut SqliteConnection,
    cost: i64,
    date: NaiveDate,
    prefix: &str,
) -> AppResult<(QuoteLine, String)> {
    let bands: Vec<(i64, i64, Option<i64>, i64, i64)> = sqlx::query_as("SELECT id,over_cents,up_to_cents,base_cents,rate_cents_per_1000 FROM fee_scale_bands WHERE scale_code=? AND effective_from<=? AND (effective_to IS NULL OR effective_to>?) ORDER BY over_cents")
        .bind(SCALE).bind(date.to_string()).bind(date.to_string()).fetch_all(&mut *tx).await?;
    let (band, over, up_to, base, rate) = bands
        .iter()
        .copied()
        .find(|(_, _, up_to, _, _)| up_to.is_none_or(|u| cost <= u))
        .ok_or_else(|| AppError::conflict(format!("No building fee scale is effective on {date}.")))?;
    let excess = (cost - over).max(0);
    let variable = ledger::amount(excess, rate, 100_000)?;
    let amount = base + variable;
    let item: i64 =
        sqlx::query_scalar("SELECT id FROM price_items WHERE code=?").bind(SCALE_ITEM).fetch_one(&mut *tx).await?;
    let range = match up_to {
        Some(u) if over == 0 => format!("up to {}", ledger::money(u)),
        Some(u) => format!("over {} up to {}", ledger::money(over), ledger::money(u)),
        None => format!("over {}", ledger::money(over)),
    };
    let explanation = if rate == 0 {
        format!(
            "{prefix}Estimated cost {} is in the band {range}: flat {}. {NOTE}.",
            ledger::money(cost),
            ledger::money(base)
        )
    } else {
        format!(
            "{prefix}Estimated cost {} is in the band {range}: {} + {} per $1,000 over {} ({} × {}/$1,000 = {}, pro rata to the cent) = {}. {NOTE}.",
            ledger::money(cost),
            ledger::money(base),
            ledger::money(rate),
            ledger::money(over),
            ledger::money(excess),
            ledger::money(rate),
            ledger::money(variable),
            ledger::money(amount)
        )
    };
    let line = QuoteLine {
        price_item_id: Some(item),
        price_version_id: None,
        item_code: SCALE_ITEM.into(),
        kind: "fee".into(),
        description: format!("Building development and works fee — estimated cost {} ({NOTE})", ledger::money(cost)),
        quantity_milli: 1000,
        quantity_minutes: None,
        unit_amount_cents: amount,
        amount_cents: amount,
        calc: json!({"rule":"building_works_scale","scale":SCALE,"band_id":band,"estimated_cost_cents":cost,"over_cents":over,"up_to_cents":up_to,"base_cents":base,"rate_cents_per_1000":rate,"variable_cents":variable}),
    };
    Ok((line, explanation))
}
async fn propose(
    tx: &mut SqliteConnection,
    case: &CaseRow,
    kind: Option<RouteKind>,
    cost: Option<i64>,
    types: &[String],
) -> AppResult<Proposal> {
    let date = pricing_date(case)?;
    if kind == Some(RouteKind::Modification) && types.len() == 1 && types[0] == "lapse_date" {
        let mut line = api::quote(tx, BASIC_ITEM, 1000, None, date).await?;
        line.calc = json!({"rule":"basic_modification","modification_types":types});
        let amount = line.amount_cents;
        return Ok(Proposal {
            rule: "basic_modification",
            lines: vec![line],
            amount,
            explanation: format!(
                "Basic modification (only the approval lapse date changes): flat {}. {NOTE}.",
                ledger::money(amount)
            ),
        });
    }
    let prefix = match kind {
        Some(RouteKind::Project) => "Building Development and Works scale. ",
        Some(RouteKind::Modification) if types.is_empty() => {
            return Err(AppError::field("modification_types", "Record the type of modification to calculate the fee."));
        }
        Some(RouteKind::Modification) => {
            "Standard modification (not only a lapse-date change) uses the Building Development and Works scale. "
        }
        None => {
            return Err(AppError::field(
                "method",
                "No schedule rule applies to this service; record a staff assessment with its basis.",
            ));
        }
    };
    let cost = cost.ok_or_else(|| {
        AppError::field("estimated_cost", "Record the total estimated cost of building and works to use the scale.")
    })?;
    let (line, explanation) = scale_line(tx, cost, date, prefix).await?;
    Ok(Proposal {
        rule: if kind == Some(RouteKind::Project) { "building_works_scale" } else { "standard_modification" },
        amount: line.amount_cents,
        lines: vec![line],
        explanation,
    })
}

/// Invoice lines for the payment step: the current assessment with approved waivers applied.
pub async fn pricing_lines(
    tx: &mut SqliteConnection,
    case: &CaseRow,
) -> AppResult<Option<(NaiveDate, Vec<QuoteLine>)>> {
    if !applies(tx, case).await? {
        return Ok(None);
    }
    let assessment = current(tx, case.id).await?.ok_or_else(|| {
        AppError::conflict("Record the fee assessment (schedule calculation or staff assessment) before invoicing.")
    })?;
    Ok(Some((pricing_date(case)?, api::apply_waivers(tx, case.id, assessment.lines).await?)))
}

/// The payment step issued the first invoice: the current assessment is now what has been charged.
pub async fn invoice_issued(tx: &mut SqliteConnection, case: &CaseRow) -> AppResult<()> {
    if let Some(a) = current(tx, case.id).await?
        && a.charged_cents.is_none()
    {
        sqlx::query("UPDATE building_fee_assessments SET charged_cents=amount_cents WHERE id=?")
            .bind(a.id)
            .execute(&mut *tx)
            .await?;
    }
    Ok(())
}

async fn has_invoice(tx: &mut SqliteConnection, case: i64) -> AppResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM invoices WHERE case_id=? AND kind='invoice' AND status='issued')",
    )
    .bind(case)
    .fetch_one(&mut *tx)
    .await?)
}
/// Extra block for the payment step: the fee invoice must exist (an unpriced case is never "settled").
pub async fn payment_block(tx: &mut SqliteConnection, case: &CaseRow) -> AppResult<Option<String>> {
    if !applies(tx, case).await? || has_invoice(tx, case.id).await? {
        return Ok(None);
    }
    Ok(Some("The fee invoice has not been issued yet. Record the fee assessment and enter the payment step.".into()))
}
/// Issue-time gate for building decisions: assessed, invoiced, and paid or explicitly waived.
pub async fn decision_block(tx: &mut SqliteConnection, case: &CaseRow) -> AppResult<Option<String>> {
    if !applies(tx, case).await? {
        return Ok(None);
    }
    if current(tx, case.id).await?.is_none() {
        return Ok(Some("Record the fee assessment before issuing decisions.".into()));
    }
    if !has_invoice(tx, case.id).await? {
        return Ok(Some("Issue the fee invoice before issuing decisions.".into()));
    }
    if !api::case_settled(tx, case.id).await? {
        return Ok(Some(
            "The fee invoice is not fully paid. Confirm payment (or a waiver approved before invoicing) before issuing decisions."
                .into(),
        ));
    }
    Ok(None)
}

#[derive(Deserialize)]
pub struct AssessInput {
    pub method: String,
    #[serde(default)]
    pub estimated_cost: Value,
    pub modification_types: Option<Vec<String>>,
    pub amount_cents: Option<i64>,
    #[serde(default)]
    pub reason: String,
    pub expected_revision: i64,
}

pub async fn record(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: &CaseRow,
    input: AssessInput,
) -> AppResult<Value> {
    if !applies(tx, case).await? {
        return Err(AppError::conflict("This request has no fee assessment step."));
    }
    if !crate::cases::workflow::is_open(case) {
        return Err(AppError::conflict("Reopen the request before assessing fees."));
    }
    let kind = building::route_kind(tx, case).await?;
    let a = answers(tx, case.id).await?;
    let applied_cost = cost_cents(a.get("estimated_cost").unwrap_or(&Value::Null))?;
    let applied_types = answer_types(&a);
    let cost = match cost_cents(&input.estimated_cost)? {
        Some(c) => Some(c),
        None => applied_cost,
    };
    let types = input.modification_types.clone().unwrap_or_else(|| applied_types.clone());
    if types.iter().any(|t| !MODIFICATION_TYPES.contains(&t.as_str())) {
        return Err(AppError::field("modification_types", "Choose recorded modification types."));
    }
    let previous = current(tx, case.id).await?;
    let reason = input.reason.trim().to_string();
    let changed_inputs = cost != applied_cost || (kind == Some(RouteKind::Modification) && types != applied_types);
    if reason.chars().count() > 2000 {
        return Err(AppError::field("reason", "Use at most 2000 characters."));
    }
    if reason.is_empty() && (input.method == "manual" || previous.is_some() || changed_inputs) {
        return Err(AppError::field(
            "reason",
            "Record the basis: a staff assessment, a re-assessment or changed inputs need a reason.",
        ));
    }
    let inputs = json!({"estimated_cost_cents":cost,"estimated_cost_source":if cost==applied_cost {"application"} else {"staff"},"application_estimated_cost_cents":applied_cost,"modification_types":if kind==Some(RouteKind::Modification){json!(types)}else{Value::Null},"modification_types_source":if types==applied_types {"application"} else {"staff"},"route":kind.map(RouteKind::as_str),"pricing_date":pricing_date(case)?.to_string()});
    let (method, rule, lines, amount, explanation) = match input.method.as_str() {
        "schedule" => {
            let p = propose(tx, case, kind, cost, &types).await?;
            ("schedule", p.rule, p.lines, p.amount, p.explanation)
        }
        "manual" => {
            let cents = input
                .amount_cents
                .filter(|c| (1..=100_000_000_000).contains(c))
                .ok_or_else(|| AppError::field("amount_cents", "Enter the assessed fee as a positive amount."))?;
            let reference = match propose(tx, case, kind, cost, &types).await {
                Ok(p) => format!("Schedule calculation for reference: {}", p.explanation),
                Err(_) => "No schedule calculation is available for these inputs.".into(),
            };
            let item: i64 = sqlx::query_scalar("SELECT id FROM price_items WHERE code=?")
                .bind(SCALE_ITEM)
                .fetch_one(&mut *tx)
                .await?;
            let line = QuoteLine {
                price_item_id: Some(item),
                price_version_id: None,
                item_code: SCALE_ITEM.into(),
                kind: "fee".into(),
                description: format!("Building fee assessed by Council staff ({NOTE})"),
                quantity_milli: 1000,
                quantity_minutes: None,
                unit_amount_cents: cents,
                amount_cents: cents,
                calc: json!({"rule":"staff_assessment","basis":reason,"assessed_by":actor.db_id()}),
            };
            (
                "manual",
                "staff_assessment",
                vec![line],
                cents,
                format!("Staff assessment: {}. Basis: {reason}. {reference}", ledger::money(cents)),
            )
        }
        _ => return Err(AppError::field("method", "Choose the schedule calculation or a staff assessment.")),
    };
    let waivers: Vec<(String, i64)> =
        sqlx::query_as("SELECT item_code,amount_cents FROM case_price_waivers WHERE case_id=?")
            .bind(case.id)
            .fetch_all(&mut *tx)
            .await?;
    for (code, cents) in waivers {
        if !lines.iter().any(|l| l.item_code == code && l.amount_cents >= cents) {
            return Err(AppError::conflict(format!(
                "An approved waiver of {} on {code} no longer fits this assessment. Keep a fee that covers the waiver.",
                ledger::money(cents)
            )));
        }
    }
    core::bump_revision(tx, case.id, Some(input.expected_revision)).await?;
    let version = previous.as_ref().map_or(1, |p| p.version + 1);
    let id: i64 = sqlx::query_scalar("INSERT INTO building_fee_assessments(case_id,version,method,rule,inputs_json,lines_json,explanation,amount_cents,reason,assessed_by,assessed_at) VALUES(?,?,?,?,?,?,?,?,?,?,?) RETURNING id")
        .bind(case.id).bind(version).bind(method).bind(rule).bind(inputs.to_string()).bind(serde_json::to_string(&lines)?).bind(&explanation).bind(amount).bind((!reason.is_empty()).then_some(&reason)).bind(actor.db_id()).bind(time::fmt(state.now())).fetch_one(&mut *tx).await?;
    let mut summary = format!("Fee assessment v{version}: {} — {explanation}", ledger::money(amount));
    if let Some(charged) = previous.as_ref().and_then(|p| p.charged_cents) {
        let delta = amount - charged;
        let (adjustment, now_charged) = if delta > 0 {
            let item: i64 = sqlx::query_scalar("SELECT id FROM price_items WHERE code=?")
                .bind(SCALE_ITEM)
                .fetch_one(&mut *tx)
                .await?;
            let line = QuoteLine {
                price_item_id: Some(item),
                price_version_id: None,
                item_code: SCALE_ITEM.into(),
                kind: "fee".into(),
                description: format!(
                    "Supplementary building fee — re-assessment v{version}: {} assessed, {} already invoiced",
                    ledger::money(amount),
                    ledger::money(charged)
                ),
                quantity_milli: 1000,
                quantity_minutes: None,
                unit_amount_cents: delta,
                amount_cents: delta,
                calc: json!({"rule":"reassessment_difference","assessment_id":id,"assessment_version":version,"previous_charged_cents":charged,"assessed_cents":amount}),
            };
            let invoice = api::issue_invoice(
                tx,
                state,
                actor,
                case.id,
                "invoice",
                time::local_date(state.now()),
                vec![line],
                Some(format!(
                    "Re-assessment v{version}: difference to the issued invoice. The original invoice is unchanged."
                )),
            )
            .await?;
            summary.push_str(&format!(" Supplementary invoice for {} issued.", ledger::money(delta)));
            (Some(invoice), amount)
        } else if delta < 0 {
            let (note, credited) = credit_difference(tx, state, actor, case.id, -delta, version).await?;
            summary.push_str(&format!(
                " Credit note for {} issued; the original invoice is unchanged.",
                ledger::money(credited)
            ));
            (note, charged - credited)
        } else {
            (None, charged)
        };
        sqlx::query("UPDATE building_fee_assessments SET charged_cents=?,adjustment_invoice_id=? WHERE id=?")
            .bind(now_charged)
            .bind(adjustment)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let data = json!({"assessment_id":id,"version":version,"method":method,"rule":rule,"amount_cents":amount,"inputs":inputs,"reason":reason});
    crate::audit::record(tx, actor.db_id(), "finance.fee_assessed", "case", Some(case.id), data.clone()).await?;
    core::append_event(tx, case.id, actor.db_id(), "finance.fee_assessed", Visibility::Applicant, &summary, data)
        .await?;
    Ok(json!({"id":id,"version":version,"amount_cents":amount}))
}

/// Credit `cents` of building fee lines (newest first), returning paid money to customer credit and
/// re-applying it to whatever remains outstanding.
async fn credit_difference(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: i64,
    cents: i64,
    version: i64,
) -> AppResult<(Option<i64>, i64)> {
    let rows = sqlx::query("SELECT l.* FROM finance_line_balances l JOIN invoices i ON i.id=l.invoice_id JOIN price_items p ON p.id=l.price_item_id WHERE i.case_id=? AND i.kind='invoice' AND i.status='issued' AND l.kind='fee' AND p.code IN (?,?) AND l.amount_cents>l.credited_cents ORDER BY l.id DESC")
        .bind(case).bind(SCALE_ITEM).bind(BASIC_ITEM).fetch_all(&mut *tx).await?;
    let note = format!("Re-assessment v{version} reduced the building fee");
    let mut remaining = cents;
    let mut by_invoice: Vec<(i64, Vec<QuoteLine>)> = vec![];
    for r in rows {
        if remaining == 0 {
            break;
        }
        let line: i64 = r.get("id");
        let take = remaining.min(r.get::<i64, _>("amount_cents") - r.get::<i64, _>("credited_cents"));
        let allocations: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM payment_allocations WHERE invoice_line_id=? AND reversed_at IS NULL")
                .bind(line)
                .fetch_all(&mut *tx)
                .await?;
        for a in allocations {
            payments::reverse(tx, actor, a, &note).await?;
        }
        let credit = QuoteLine {
            price_item_id: r.get("price_item_id"),
            price_version_id: r.get("price_version_id"),
            item_code: String::new(),
            kind: "fee".into(),
            description: r.get("description"),
            quantity_milli: 1000,
            quantity_minutes: None,
            unit_amount_cents: take,
            amount_cents: take,
            calc: json!({"original_line_id":line,"reason":note}),
        };
        let invoice: i64 = r.get("invoice_id");
        match by_invoice.iter_mut().find(|(i, _)| *i == invoice) {
            Some((_, lines)) => lines.push(credit),
            None => by_invoice.push((invoice, vec![credit])),
        }
        remaining -= take;
    }
    let mut last = None;
    for (invoice, lines) in by_invoice {
        let id = api::persist_invoice(
            tx,
            actor,
            case,
            "credit_note",
            time::local_date(state.now()),
            &lines,
            Some(&format!("{note}. The original invoice is unchanged.")),
            Some(invoice),
        )
        .await?;
        api::attach_pdf(tx, state, actor, id).await?;
        last = Some(id);
    }
    let credits:Vec<i64>=sqlx::query_scalar("SELECT p.id FROM payments p JOIN finance_payment_balances b ON b.payment_id=p.id WHERE p.case_id=? AND b.credit_cents>0 AND p.status='confirmed'").bind(case).fetch_all(&mut *tx).await?;
    for payment in credits {
        payments::allocate_credit(tx, actor, payment, case, None).await?;
    }
    Ok((last, cents - remaining))
}

pub async fn view(tx: &mut SqliteConnection, actor: &Actor, case: &CaseRow, access: CaseAccess) -> AppResult<Value> {
    if !applies(tx, case).await? {
        return Ok(json!({"applies":false}));
    }
    let kind = building::route_kind(tx, case).await?;
    let a = answers(tx, case.id).await?;
    let cost = cost_cents(a.get("estimated_cost").unwrap_or(&Value::Null)).unwrap_or(None);
    let types = answer_types(&a);
    let (proposal, proposal_error) = match propose(tx, case, kind, cost, &types).await {
        Ok(p) => {
            (json!({"rule":p.rule,"amount_cents":p.amount,"explanation":p.explanation,"lines":p.lines}), Value::Null)
        }
        Err(e) => (Value::Null, json!(e.fields.values().next().cloned().unwrap_or(e.message))),
    };
    let rows = sqlx::query("SELECT a.*,u.display_name FROM building_fee_assessments a LEFT JOIN users u ON u.id=a.assessed_by WHERE a.case_id=? ORDER BY a.version DESC").bind(case.id).fetch_all(&mut *tx).await?;
    let mut assessments = vec![];
    for r in rows {
        let adjustment: Option<i64> = r.get("adjustment_invoice_id");
        let adjustment = match adjustment {
            Some(id) => {
                let (number, kind, total): (String, String, i64) =
                    sqlx::query_as("SELECT number,kind,total_cents FROM invoices WHERE id=?")
                        .bind(id)
                        .fetch_one(&mut *tx)
                        .await?;
                json!({"id":id,"number":number,"kind":kind,"total_cents":total})
            }
            None => Value::Null,
        };
        assessments.push(json!({"id":r.get::<i64,_>("id"),"version":r.get::<i64,_>("version"),"method":r.get::<String,_>("method"),"rule":r.get::<String,_>("rule"),"inputs":serde_json::from_str::<Value>(&r.get::<String,_>("inputs_json"))?,"explanation":r.get::<String,_>("explanation"),"amount_cents":r.get::<i64,_>("amount_cents"),"reason":r.get::<Option<String>,_>("reason"),"assessed_by":r.get::<Option<String>,_>("display_name"),"assessed_at":r.get::<String,_>("assessed_at"),"charged_cents":r.get::<Option<i64>,_>("charged_cents"),"adjustment":adjustment}));
    }
    let waivers: Vec<(String, i64, String, String)> = sqlx::query_as("SELECT w.item_code,w.amount_cents,w.reason,u.display_name FROM case_price_waivers w JOIN users u ON u.id=w.approved_by WHERE w.case_id=?").bind(case.id).fetch_all(&mut *tx).await?;
    let roles = actor.roles_for_service(case.service_id);
    let can_assess = access.can_manage()
        && crate::cases::workflow::is_open(case)
        && roles.iter().any(|r| matches!(r, Role::Intake | Role::Specialist | Role::Finance | Role::Manager));
    Ok(
        json!({"applies":true,"route":kind.map(RouteKind::as_str),"schedule_note":NOTE,"application":{"estimated_cost_cents":cost,"modification_types":types},"proposal":proposal,"proposal_error":proposal_error,"assessments":assessments,"invoiced":has_invoice(tx,case.id).await?,"settled":api::case_settled(tx,case.id).await?,"waivers":waivers.into_iter().map(|(c,a,r,n)|json!({"item_code":c,"amount_cents":a,"reason":r,"approved_by":n})).collect::<Vec<_>>(),"can_assess":can_assess}),
    )
}

pub async fn get(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut c = state.db.acquire().await?;
    let (case, access) = authz::require_case(&mut c, &actor, id).await?;
    if matches!(access, CaseAccess::TaskOnly) {
        return Err(AppError::not_found());
    }
    Ok(Json(view(&mut c, &actor, &case, access).await?))
}
pub async fn post(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<AssessInput>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&state.db).await?;
    let (case, access) = authz::require_staff_case(&mut tx, &actor, id).await?;
    if !access.can_manage()
        || !actor
            .roles_for_service(case.service_id)
            .iter()
            .any(|r| matches!(r, Role::Intake | Role::Specialist | Role::Finance | Role::Manager))
    {
        return Err(AppError::forbidden_msg("Intake, planning, finance or manager staff record fee assessments."));
    }
    let out = record(&mut tx, &state, &actor, &case, input).await?;
    tx.commit().await?;
    Ok(Json(out))
}
