use crate::{
    auth::Actor,
    authz::{self, CaseAccess, Role},
    cases::core::{self, CaseRow, Visibility},
    error::{AppError, AppResult},
    notify::{self, Notice},
    time,
};
use chrono::{DateTime, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;

pub const CONDITIONS: &str = "Loud music must stop by 10 pm unless Council agrees otherwise. Return keys and remove your property by noon on the first business day after hire. Public liability cover of at least $20 million is required for clubs, associations and commercial hirers; casual hirers require Council's agreement. Meetings require more than 7 days' cancellation notice; weddings, concerts, stage shows and balls require 30 days. Late cancellation fees are subject to a finance decision. Leave rooms clean, take rubbish away and report risks or incidents. All times are Pacific/Norfolk. FY2026-27 schedule (demo copy — confirm with Council). Demo fees are per calendar day touched.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slot {
    pub unit_code: String,
    pub start_at: String,
    pub end_at: String,
    pub attendees: i64,
}
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Resource {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub kind: String,
    pub venue: Option<String>,
    pub description: String,
    pub capacity: Option<i64>,
    pub prep_minutes: i64,
    pub cleanup_minutes: i64,
    pub price_item_code: Option<String>,
    pub active: i64,
}
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Unit {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub venue: String,
    pub description: String,
    pub fee_item_code: Option<String>,
    pub deposit_item_code: Option<String>,
    pub active: i64,
}
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Booking {
    pub id: i64,
    pub case_id: i64,
    pub unit_id: i64,
    pub status: String,
    pub start_at: String,
    pub end_at: String,
    pub attendees: Option<i64>,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
    pub confirmation_version_id: Option<i64>,
}
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Conflict {
    pub resource_id: i64,
    pub source: String,
    pub label: String,
    pub case_id: Option<i64>,
    pub reference: Option<String>,
    pub start_at: String,
    pub end_at: String,
}

pub async fn unit(tx: &mut SqliteConnection, code: &str) -> AppResult<Unit> {
    let u: Unit = sqlx::query_as("SELECT * FROM bookable_units WHERE code=? AND active=1")
        .bind(code)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::field("unit_code", "Choose an active space."))?;
    let resources = unit_resources(tx, u.id).await?;
    if resources.is_empty() || resources.iter().any(|r| r.active != 1) {
        return Err(AppError::field("unit_code", "This space is unavailable."));
    }
    Ok(u)
}
pub async fn unit_resources(tx: &mut SqliteConnection, id: i64) -> AppResult<Vec<Resource>> {
    Ok(sqlx::query_as("SELECT r.* FROM resources r JOIN bookable_unit_resources ur ON ur.resource_id=r.id WHERE ur.unit_id=? ORDER BY r.id").bind(id).fetch_all(&mut *tx).await?)
}
pub async fn booking(tx: &mut SqliteConnection, case: i64) -> AppResult<Booking> {
    Ok(sqlx::query_as("SELECT * FROM bookings WHERE case_id=?").bind(case).fetch_one(&mut *tx).await?)
}

pub fn slot_times(s: &Slot, now: DateTime<Utc>) -> AppResult<(DateTime<Utc>, DateTime<Utc>)> {
    let start =
        time::parse(&s.start_at).map_err(|_| AppError::field("start_at", "Use a valid RFC 3339 UTC start time."))?;
    let end = time::parse(&s.end_at).map_err(|_| AppError::field("end_at", "Use a valid RFC 3339 UTC end time."))?;
    if !s.start_at.ends_with('Z') || !s.end_at.ends_with('Z') {
        return Err(AppError::field("start_at", "Supply UTC times ending in Z."));
    }
    if start <= now {
        return Err(AppError::field("start_at", "Choose a future start time."));
    }
    if end <= start || end - start > Duration::hours(12) {
        return Err(AppError::field("end_at", "Hire must last between one minute and 12 hours."));
    }
    if end - start < Duration::minutes(1) {
        return Err(AppError::field("end_at", "Hire must last at least one minute."));
    }
    let local_start = time::to_local(start);
    let local_end = time::to_local(end);
    let midnight_next = local_start.date_naive().succ_opt() == Some(local_end.date_naive())
        && local_end.time().num_seconds_from_midnight() == 0
        && local_end.nanosecond() == 0;
    if local_start.hour() < 7 || !(local_end.date_naive() == local_start.date_naive() || midnight_next) {
        return Err(AppError::field(
            "end_at",
            "Choose times between 07:00 and midnight Norfolk time. Loud music stops by 10 pm.",
        ));
    }
    if !(1..=10000).contains(&s.attendees) {
        return Err(AppError::field(
            "attendees",
            "Enter a positive guest count; confirm the room capacity with Council.",
        ));
    }
    Ok((start, end))
}
pub async fn validate_slot(tx: &mut SqliteConnection, s: &Slot, now: DateTime<Utc>) -> AppResult<Unit> {
    slot_times(s, now)?;
    let u = unit(tx, &s.unit_code).await?;
    let resources = unit_resources(tx, u.id).await?;
    if let Some(capacity) =
        resources.iter().map(|r| r.capacity).collect::<Option<Vec<_>>>().map(|c| c.iter().sum::<i64>())
        && s.attendees > capacity
    {
        return Err(AppError::field("attendees", format!("The space allows at most {capacity} guests.")));
    }
    Ok(u)
}
pub fn day_count(start: &str, end: &str) -> AppResult<i64> {
    let a = time::local_date(time::parse(start)?);
    // Half-open booking interval: midnight at the end does not touch the next day.
    let b = time::local_date(time::parse(end)? - Duration::milliseconds(1));
    Ok((b - a).num_days() + 1)
}
pub async fn conflicts(
    tx: &mut SqliteConnection,
    u: i64,
    start: &str,
    end: &str,
    exclude: Option<i64>,
) -> AppResult<Vec<Conflict>> {
    let start = time::parse(start)?;
    let end = time::parse(end)?;
    let mut out = Vec::new();
    for r in unit_resources(tx, u).await? {
        let a = time::fmt(start - Duration::minutes(r.prep_minutes));
        let b = time::fmt(end + Duration::minutes(r.cleanup_minutes));
        let mut rows:Vec<Conflict>=sqlx::query_as("SELECT o.resource_id,o.source,o.label,o.case_id,c.number AS reference,o.start_at,o.end_at FROM occupancies o LEFT JOIN cases c ON c.id=o.case_id WHERE o.resource_id=? AND o.active=1 AND o.start_at<? AND o.end_at>? AND (? IS NULL OR o.booking_id IS NULL OR o.booking_id<>?)")
            .bind(r.id).bind(&b).bind(&a).bind(exclude).bind(exclude).fetch_all(&mut *tx).await?;
        out.append(&mut rows);
    }
    Ok(out)
}
/// Call inside BEGIN IMMEDIATE; the trigger is a second line of defence.
pub async fn occupy_booking(tx: &mut SqliteConnection, b: &Booking, u: &Unit, label: &str) -> AppResult<()> {
    let busy = conflicts(tx, u.id, &b.start_at, &b.end_at, Some(b.id)).await?;
    if !busy.is_empty() {
        let refs = busy.iter().map(|c| c.reference.as_deref().unwrap_or(&c.label)).collect::<Vec<_>>().join(", ");
        return Err(AppError::conflict(format!("That time conflicts with {refs}.")));
    }
    for r in unit_resources(tx, u.id).await? {
        sqlx::query("INSERT INTO occupancies(resource_id,source,booking_id,case_id,label,start_at,end_at,created_at) VALUES (?,'booking',?,?,?,?,?,?)")
            .bind(r.id).bind(b.id).bind(b.case_id).bind(label).bind(time::fmt(time::parse(&b.start_at)?-Duration::minutes(r.prep_minutes))).bind(time::fmt(time::parse(&b.end_at)?+Duration::minutes(r.cleanup_minutes))).bind(time::now_str()).execute(&mut *tx).await?;
    }
    Ok(())
}
pub async fn revision(tx: &mut SqliteConnection, b: &Booking, actor: Option<i64>, reason: &str) -> AppResult<()> {
    sqlx::query("INSERT INTO booking_revisions(booking_id,revision,unit_id,start_at,end_at,status,changed_by,reason,created_at) VALUES (?,?,?,?,?,?,?,?,?)")
        .bind(b.id).bind(b.revision).bind(b.unit_id).bind(&b.start_at).bind(&b.end_at).bind(&b.status).bind(actor).bind(reason).bind(time::now_str()).execute(&mut *tx).await?;
    Ok(())
}
pub async fn record(
    tx: &mut SqliteConnection,
    actor: Option<i64>,
    case: i64,
    action: &str,
    summary: &str,
    data: Value,
) -> AppResult<()> {
    core::append_event(tx, case, actor, action, Visibility::Applicant, summary, data.clone()).await?;
    crate::audit::record(tx, actor, action, "case", Some(case), data).await
}
pub async fn manage(tx: &mut SqliteConnection, a: &Actor, id: i64, roles: &[Role]) -> AppResult<CaseRow> {
    let (c, access) = authz::require_staff_case(tx, a, id).await?;
    if !access.can_manage() || !a.roles_for_service(c.service_id).iter().any(|r| roles.contains(r)) {
        return Err(AppError::forbidden());
    }
    if matches!(c.status.as_str(), "completed" | "cancelled" | "withdrawn" | "refused" | "closed_duplicate") {
        return Err(AppError::conflict("This request is closed."));
    }
    Ok(c)
}
pub async fn read_case(tx: &mut SqliteConnection, a: &Actor, id: i64) -> AppResult<(CaseRow, CaseAccess)> {
    let (c, access) = authz::require_case(tx, a, id).await?;
    if access == CaseAccess::TaskOnly {
        return Err(AppError::not_found());
    }
    Ok((c, access))
}
pub async fn applicant_notice(tx: &mut SqliteConnection, c: &CaseRow, subject: &str, body: &str) -> AppResult<()> {
    notify::send(
        tx,
        Notice {
            user_id: c.applicant_user_id,
            email: c.applicant_email.clone(),
            case_id: Some(c.id),
            subject: subject.into(),
            body: body.into(),
            link: Some(format!("/my/cases/{}", c.id)),
            ..Notice::default()
        },
    )
    .await
}
pub async fn role_notice(tx: &mut SqliteConnection, case: i64, role: &str, subject: &str, body: &str) -> AppResult<()> {
    let users:Vec<i64>=sqlx::query_scalar("SELECT DISTINCT g.user_id FROM role_grants g JOIN users u ON u.id=g.user_id JOIN cases c ON c.id=? WHERE u.is_active=1 AND g.role=? AND g.revoked_at IS NULL AND (g.scope_service_id IS NULL OR g.scope_service_id=c.service_id)").bind(case).bind(role).fetch_all(&mut *tx).await?;
    for id in users {
        let Some(actor) = Actor::load_recipient(tx, id).await? else { continue };
        if authz::case_access(tx, &actor, case).await?.is_staff() {
            notify::send(
                tx,
                Notice {
                    user_id: Some(id),
                    case_id: Some(case),
                    subject: subject.into(),
                    body: body.into(),
                    link: Some(format!("/staff/cases/{case}")),
                    ..Notice::default()
                },
            )
            .await?;
        }
    }
    Ok(())
}
pub fn text<'a>(v: &'a Value, key: &str, max: usize) -> AppResult<&'a str> {
    let s = v[key].as_str().unwrap_or("").trim();
    if s.is_empty() || s.len() > max {
        return Err(AppError::field(key, format!("Enter between 1 and {max} characters.")));
    }
    Ok(s)
}
pub async fn answers(tx: &mut SqliteConnection, id: i64) -> AppResult<Value> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT answers_json FROM submissions WHERE case_id=? ORDER BY id DESC LIMIT 1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    Ok(raw.map(|s| serde_json::from_str(&s)).transpose()?.unwrap_or(json!({})))
}

/// Calendar facts remain visible; references and labels require access to the conflicting case too.
pub async fn visible_conflicts(
    tx: &mut SqliteConnection,
    a: &Actor,
    mut rows: Vec<Conflict>,
) -> AppResult<Vec<Conflict>> {
    for row in &mut rows {
        if let Some(id) = row.case_id
            && !authz::case_access(tx, a, id).await?.is_staff()
        {
            row.case_id = None;
            row.reference = None;
            row.label = "Unavailable".into();
        }
    }
    Ok(rows)
}
pub async fn ensure_available(
    tx: &mut SqliteConnection,
    a: &Actor,
    u: i64,
    start: &str,
    end: &str,
    exclude: Option<i64>,
) -> AppResult<()> {
    let rows = conflicts(tx, u, start, end, exclude).await?;
    let rows = visible_conflicts(tx, a, rows).await?;
    if rows.is_empty() {
        return Ok(());
    }
    Err(AppError::conflict(format!(
        "That time conflicts with {}.",
        rows.iter().map(|r| r.reference.as_deref().unwrap_or(&r.label)).collect::<Vec<_>>().join(", ")
    )))
}
