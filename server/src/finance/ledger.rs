use crate::{
    audit,
    auth::Actor,
    cases::core::{self, Visibility},
    error::{AppError, AppResult},
    notify::{self, Notice},
    time,
};
use serde_json::{Value, json};
use sqlx::SqliteConnection;

pub fn money(cents: i64) -> String {
    format!("${}.{:02}", cents / 100, cents.abs() % 100)
}
pub fn amount(qty: i64, rate: i64, denominator: i64) -> AppResult<i64> {
    if qty < 0 || rate < 0 {
        return Err(AppError::field("amount_cents", "Amounts and quantities cannot be negative."));
    }
    i64::try_from((i128::from(qty) * i128::from(rate) + i128::from(denominator / 2)) / i128::from(denominator))
        .map_err(|_| AppError::field("amount_cents", "Amount is too large."))
}
/// One idempotent balanced journal entry, including all components of a money event.
pub async fn post(
    tx: &mut SqliteConnection,
    case: Option<i64>,
    source: &str,
    id: i64,
    purpose: &str,
    memo: &str,
    lines: &[(&str, i64, i64)],
) -> AppResult<()> {
    let debit: i128 = lines.iter().map(|l| i128::from(l.1)).sum();
    let credit: i128 = lines.iter().map(|l| i128::from(l.2)).sum();
    if debit != credit || lines.iter().any(|l| l.1 < 0 || l.2 < 0 || (l.1 > 0 && l.2 > 0)) {
        return Err(AppError::internal("Unbalanced journal entry"));
    }
    if debit == 0 {
        return Ok(());
    }
    let entry: Option<i64> = sqlx::query_scalar("INSERT INTO journal_entries(at,case_id,source_type,source_id,purpose,memo) VALUES(?,?,?,?,?,?) ON CONFLICT(source_type,source_id,purpose) DO NOTHING RETURNING id")
        .bind(time::now_str()).bind(case).bind(source).bind(id).bind(purpose).bind(memo).fetch_optional(&mut *tx).await?;
    if let Some(entry) = entry {
        for (account, dr, cr) in lines.iter().filter(|l| l.1 + l.2 > 0) {
            sqlx::query("INSERT INTO journal_lines(entry_id,account,debit_cents,credit_cents) VALUES(?,?,?,?)")
                .bind(entry)
                .bind(account)
                .bind(dr)
                .bind(cr)
                .execute(&mut *tx)
                .await?;
        }
    }
    Ok(())
}
/// Records a finance timeline event shown to the applicant and to staff.
pub async fn event(
    tx: &mut SqliteConnection,
    actor: &Actor,
    case: i64,
    kind: &str,
    summary: &str,
    data: Value,
) -> AppResult<()> {
    audit::record(tx, actor.db_id(), kind, "case", Some(case), data.clone()).await?;
    core::append_event(tx, case, actor.db_id(), kind, Visibility::Applicant, summary, data).await?;
    Ok(())
}
/// Records a finance event whose detail is an internal accounting note: one timeline event whose stored summary is
/// the plain `applicant_summary`; staff read `staff_note` instead (`data.staff_note`, rendered by
/// `cases::timeline::event_summary`; event data never reaches the applicant).
pub async fn event_with_note(
    tx: &mut SqliteConnection,
    actor: &Actor,
    case: i64,
    kind: &str,
    applicant_summary: &str,
    staff_note: &str,
    data: Value,
) -> AppResult<()> {
    let mut data = data;
    if let Some(object) = data.as_object_mut() {
        object.insert("staff_note".into(), Value::String(staff_note.into()));
    } else {
        data = serde_json::json!({ "detail": data, "staff_note": staff_note });
    }
    audit::record(tx, actor.db_id(), kind, "case", Some(case), data.clone()).await?;
    core::append_event(tx, case, actor.db_id(), kind, Visibility::Applicant, applicant_summary, data).await?;
    Ok(())
}
/// A notification's subject and body.
pub type Message<'a> = (&'a str, &'a str);
/// Notifies the applicant side with `applicant` wording (second person) and the case owners with
/// `staff` wording (third person, about the applicant).
pub async fn tell(tx: &mut SqliteConnection, case: i64, applicant: Message<'_>, staff: Message<'_>) -> AppResult<()> {
    let (subject, body) = applicant;
    let c = core::load_case(tx, case).await?;
    let recipients:Vec<(i64,String)>=sqlx::query_as("SELECT u.id,u.email FROM users u WHERE u.is_active=1 AND (u.id=? OR u.id IN (SELECT user_id FROM memberships WHERE organisation_id=? AND status='active') OR u.id IN (SELECT user_id FROM case_representatives WHERE case_id=? AND status='active'))")
        .bind(c.applicant_user_id).bind(c.applicant_org_id).bind(case).fetch_all(&mut *tx).await?;
    for (uid, email) in recipients {
        let Some(recipient) = Actor::load_recipient(tx, uid).await? else { continue };
        if matches!(
            crate::authz::case_access(tx, &recipient, case).await?,
            crate::authz::CaseAccess::Applicant | crate::authz::CaseAccess::Staff { .. }
        ) {
            notify::send(
                tx,
                Notice {
                    user_id: Some(uid),
                    email: Some(email),
                    case_id: Some(case),
                    subject: subject.into(),
                    body: body.into(),
                    link: Some(format!("/my/cases/{case}?tab=finance.money")),
                    ..Default::default()
                },
            )
            .await?;
        }
    }
    if c.applicant_user_id.is_none() && c.applicant_org_id.is_none() {
        notify::send(
            tx,
            Notice {
                email: c.applicant_email,
                phone: c.applicant_phone,
                case_id: Some(case),
                subject: subject.into(),
                body: body.into(),
                link: Some(format!("/my/cases/{case}?tab=finance.money")),
                ..Default::default()
            },
        )
        .await?;
    }
    let owners: Vec<i64> = sqlx::query_scalar(
        "SELECT a.user_id FROM case_assignments a JOIN users u ON u.id=a.user_id WHERE a.case_id=? AND a.role='owner' AND a.ended_at IS NULL AND u.is_active=1",
    )
    .bind(case)
    .fetch_all(&mut *tx)
    .await?;
    for owner in owners {
        let owner_actor = Actor::load(tx, owner, true).await?;
        if !crate::authz::case_access(tx, &owner_actor, case).await?.is_staff() {
            continue;
        }
        notify::send(
            tx,
            Notice {
                user_id: Some(owner),
                case_id: Some(case),
                subject: staff.0.into(),
                body: staff.1.into(),
                link: Some(format!("/staff/cases/{case}?tab=finance.money")),
                ..Default::default()
            },
        )
        .await?;
    }
    Ok(())
}
pub async fn finance_notice(tx: &mut SqliteConnection, case: i64, subject: &str) -> AppResult<()> {
    let users: Vec<i64> =
        sqlx::query_scalar("SELECT DISTINCT g.user_id FROM role_grants g JOIN users u ON u.id=g.user_id WHERE g.role='finance' AND g.revoked_at IS NULL AND u.is_active=1")
            .fetch_all(&mut *tx)
            .await?;
    for uid in users {
        let Some(actor) = Actor::load_recipient(tx, uid).await? else { continue };
        let access = crate::authz::case_access(tx, &actor, case).await?;
        if access.is_staff() {
            notify::send(
                tx,
                Notice {
                    user_id: Some(uid),
                    case_id: Some(case),
                    subject: subject.into(),
                    body: subject.into(),
                    link: Some("/staff/finance/refunds".into()),
                    ..Default::default()
                },
            )
            .await?;
        }
    }
    Ok(())
}
pub async fn journal(tx: &mut SqliteConnection, case: Option<i64>) -> AppResult<Value> {
    let rows = sqlx::query("SELECT e.id,e.at,e.case_id,e.source_type source,e.memo,l.account,l.debit_cents,l.credit_cents FROM journal_entries e JOIN journal_lines l ON l.entry_id=e.id WHERE (? IS NULL OR e.case_id=?) ORDER BY e.id DESC,l.id")
        .bind(case).bind(case).fetch_all(&mut *tx).await?;
    let rows: Vec<Value> = rows.iter().map(super::views::row).collect();
    let mut accounts = std::collections::BTreeMap::<String, (i64, i64)>::new();
    for r in &rows {
        let a = accounts.entry(r["account"].as_str().unwrap_or_default().into()).or_default();
        a.0 += r["debit_cents"].as_i64().unwrap_or(0);
        a.1 += r["credit_cents"].as_i64().unwrap_or(0);
    }
    let balance: Vec<Value> = accounts.into_iter().map(|(account,(debit_cents,credit_cents))|json!({"account":account,"debit_cents":debit_cents,"credit_cents":credit_cents,"balance_cents":debit_cents-credit_cents})).collect();
    Ok(json!({"entries":rows,"trial_balance":balance}))
}
