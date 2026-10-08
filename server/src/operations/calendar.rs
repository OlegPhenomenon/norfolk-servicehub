use super::model::{self, Resource, Unit};
use crate::{
    auth::StaffActor,
    authz::{self, Role},
    db,
    error::{AppError, AppResult},
    state::AppState,
    time,
    web::{Json, Path, Query},
};
use axum::extract::State;
use chrono::{Duration, NaiveTime};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Deserialize)]
pub struct Range {
    from: String,
    to: String,
    resource: Option<String>,
}
fn instant(s: &str) -> AppResult<String> {
    if s.len() == 10 {
        Ok(time::fmt(time::local_to_utc(time::parse_date(s)?, NaiveTime::MIN)))
    } else {
        Ok(time::fmt(time::parse(s)?))
    }
}
fn range(v: &Range) -> AppResult<(String, String)> {
    let from = instant(&v.from).map_err(|_| AppError::field("from", "Choose a valid start date."))?;
    let to = instant(&v.to).map_err(|_| AppError::field("to", "Choose a valid end date."))?;
    let duration = time::parse(&to)? - time::parse(&from)?;
    if duration <= Duration::zero() || duration > Duration::days(93) {
        return Err(AppError::field("to", "Choose a range of at most 93 days."));
    }
    Ok((from, to))
}
#[derive(Serialize, sqlx::FromRow)]
struct Busy {
    start_at: String,
    end_at: String,
    source: String,
}
pub async fn availability(State(st): State<AppState>, Query(v): Query<Range>) -> AppResult<Json<Value>> {
    let (from, to) = range(&v)?;
    let mut tx = st.db.acquire().await?;
    let units: Vec<Unit> =
        sqlx::query_as("SELECT * FROM bookable_units WHERE venue='Rawson Hall' AND active=1 ORDER BY id")
            .fetch_all(&mut *tx)
            .await?;
    let mut out = Vec::new();
    for u in units {
        let resources = model::unit_resources(&mut tx, u.id).await?;
        let busy:Vec<Busy>=sqlx::query_as("SELECT DISTINCT o.start_at,o.end_at,o.source FROM occupancies o JOIN bookable_unit_resources ur ON ur.resource_id=o.resource_id WHERE ur.unit_id=? AND o.active=1 AND o.start_at<? AND o.end_at>? ORDER BY o.start_at").bind(u.id).bind(&to).bind(&from).fetch_all(&mut *tx).await?;
        out.push(json!({"code":u.code,"name":u.name,"active":resources.iter().all(|r|r.active==1),"capacity":resources.iter().map(|r|r.capacity).collect::<Option<Vec<_>>>().map(|c|c.iter().sum::<i64>()),"prep_minutes":resources.iter().map(|r|r.prep_minutes).max().unwrap_or(0),"cleanup_minutes":resources.iter().map(|r|r.cleanup_minutes).max().unwrap_or(0),"busy":busy.into_iter().map(|b|json!({"start_at":b.start_at,"end_at":b.end_at,"label":if b.source=="maintenance"{"Unavailable"}else{"Booked"}})).collect::<Vec<_>>()}));
    }
    Ok(Json(json!({"units":out,"timezone":"Pacific/Norfolk","conditions":model::CONDITIONS})))
}
#[derive(Serialize, sqlx::FromRow)]
struct Entry {
    id: i64,
    resource_id: i64,
    source: String,
    label: String,
    case_id: Option<i64>,
    reference: Option<String>,
    start_at: String,
    end_at: String,
    event_start: Option<String>,
    event_end: Option<String>,
    booking_id: Option<i64>,
}
#[derive(Serialize, sqlx::FromRow)]
struct RequestEntry {
    id: i64,
    case_id: i64,
    unit_code: String,
    unit_name: String,
    title: String,
    start_at: String,
    end_at: String,
}
pub async fn staff_calendar(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Query(v): Query<Range>,
) -> AppResult<Json<Value>> {
    a.require_any_role(&[Role::Intake, Role::Specialist, Role::Manager])?;
    let (from, to) = range(&v)?;
    let mut tx = st.db.acquire().await?;
    let resources: Vec<Resource> =
        sqlx::query_as("SELECT * FROM resources WHERE (? IS NULL OR code=? OR CAST(id AS TEXT)=?) ORDER BY id")
            .bind(&v.resource)
            .bind(&v.resource)
            .bind(&v.resource)
            .fetch_all(&mut *tx)
            .await?;
    let rows:Vec<Entry>=sqlx::query_as("SELECT o.id,o.resource_id,o.source,o.label,o.case_id,c.number AS reference,o.start_at,o.end_at,b.start_at AS event_start,b.end_at AS event_end,o.booking_id FROM occupancies o LEFT JOIN bookings b ON b.id=o.booking_id LEFT JOIN cases c ON c.id=o.case_id JOIN resources r ON r.id=o.resource_id WHERE o.active=1 AND o.start_at<? AND o.end_at>? AND (? IS NULL OR r.code=? OR CAST(r.id AS TEXT)=?) ORDER BY o.start_at")
        .bind(&to).bind(&from).bind(&v.resource).bind(&v.resource).bind(&v.resource).fetch_all(&mut *tx).await?;
    let mut entries = Vec::new();
    for mut row in rows {
        if let Some(case) = row.case_id
            && !authz::case_access(&mut tx, &a, case).await?.is_staff()
        {
            row.case_id = None;
            row.reference = None;
            row.label = "Unavailable".into();
            row.event_start = None;
            row.event_end = None;
            row.booking_id = None;
        }
        entries.push(row);
    }
    let rows:Vec<RequestEntry>=sqlx::query_as("SELECT b.id,b.case_id,u.code AS unit_code,u.name AS unit_name,c.title,b.start_at,b.end_at FROM bookings b JOIN bookable_units u ON u.id=b.unit_id JOIN cases c ON c.id=b.case_id WHERE b.status='requested' AND b.start_at<? AND b.end_at>? AND (? IS NULL OR EXISTS(SELECT 1 FROM bookable_unit_resources ur JOIN resources r ON r.id=ur.resource_id WHERE ur.unit_id=u.id AND (r.code=? OR CAST(r.id AS TEXT)=?))) ORDER BY b.start_at")
        .bind(&to).bind(&from).bind(&v.resource).bind(&v.resource).bind(&v.resource).fetch_all(&mut *tx).await?;
    let mut requests = Vec::new();
    for row in rows {
        if authz::case_access(&mut tx, &a, row.case_id).await?.is_staff() {
            requests.push(row);
        }
    }
    Ok(Json(json!({"resources":resources,"entries":entries,"requests":requests})))
}
pub async fn resources(State(st): State<AppState>, StaffActor(a): StaffActor) -> AppResult<Json<Value>> {
    a.require_any_role(&[Role::Sysadmin, Role::Manager])?;
    let rows: Vec<Resource> = sqlx::query_as("SELECT * FROM resources ORDER BY id").fetch_all(&st.db).await?;
    Ok(Json(json!(rows)))
}
#[derive(Serialize, sqlx::FromRow)]
struct Worker {
    id: i64,
    name: String,
}
pub async fn staff_resources(State(st): State<AppState>, StaffActor(a): StaffActor) -> AppResult<Json<Value>> {
    a.require_any_role(&[Role::Intake, Role::Specialist, Role::Manager])?;
    let rows: Vec<Resource> =
        sqlx::query_as("SELECT * FROM resources WHERE active=1 ORDER BY id").fetch_all(&st.db).await?;
    let workers:Vec<Worker>=sqlx::query_as("SELECT DISTINCT u.id,u.display_name AS name FROM users u JOIN role_grants g ON g.user_id=u.id WHERE u.is_active=1 AND u.kind='staff' AND g.role='field_worker' AND g.revoked_at IS NULL ORDER BY u.display_name").fetch_all(&st.db).await?;
    Ok(Json(json!({"resources":rows,"workers":workers})))
}
#[derive(Deserialize)]
pub struct ResourceUpdate {
    name: String,
    prep_minutes: i64,
    cleanup_minutes: i64,
    active: bool,
}
pub async fn update_resource(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path(id): Path<i64>,
    Json(v): Json<ResourceUpdate>,
) -> AppResult<Json<Value>> {
    a.require_any_role(&[Role::Sysadmin, Role::Manager])?;
    if v.name.trim().is_empty() || v.name.len() > 120 {
        return Err(AppError::field("name", "Enter a name up to 120 characters."));
    }
    if !(0..=1440).contains(&v.prep_minutes) || !(0..=1440).contains(&v.cleanup_minutes) {
        return Err(AppError::field("prep_minutes", "Buffers must be between 0 and 1440 minutes."));
    }
    let mut tx = db::write_tx(&st.db).await?;
    let changed = sqlx::query("UPDATE resources SET name=?,prep_minutes=?,cleanup_minutes=?,active=? WHERE id=?")
        .bind(&v.name)
        .bind(v.prep_minutes)
        .bind(v.cleanup_minutes)
        .bind(i64::from(v.active))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if changed.rows_affected() == 0 {
        return Err(AppError::not_found());
    }
    crate::audit::record(
        &mut tx,
        a.db_id(),
        "resource.updated",
        "resource",
        Some(id),
        json!({"name":v.name,"prep_minutes":v.prep_minutes,"cleanup_minutes":v.cleanup_minutes,"active":v.active}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"status":"updated"})))
}
#[derive(Deserialize)]
pub struct Maintenance {
    resource_code: String,
    start: String,
    end: String,
    label: String,
}
pub async fn maintenance(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Json(v): Json<Maintenance>,
) -> AppResult<Json<Value>> {
    a.require_any_role(&[Role::Sysadmin, Role::Manager])?;
    let start = instant(&v.start).map_err(|_| AppError::field("start", "Enter a valid start time."))?;
    let end = instant(&v.end).map_err(|_| AppError::field("end", "Enter a valid end time."))?;
    if end <= start {
        return Err(AppError::field("end", "End must be after start."));
    }
    if v.label.trim().is_empty() || v.label.len() > 200 {
        return Err(AppError::field("label", "Explain the maintenance (up to 200 characters)."));
    }
    let mut tx = db::write_tx(&st.db).await?;
    let resource: i64 =
        sqlx::query_scalar("SELECT id FROM resources WHERE code=?").bind(&v.resource_code).fetch_one(&mut *tx).await?;
    let conflict:Option<(Option<i64>,String)>=sqlx::query_as("SELECT o.case_id,COALESCE(c.number,o.label) FROM occupancies o LEFT JOIN cases c ON c.id=o.case_id WHERE resource_id=? AND active=1 AND start_at<? AND end_at>? LIMIT 1").bind(resource).bind(&end).bind(&start).fetch_optional(&mut *tx).await?;
    if let Some((case_id, label)) = conflict {
        let visible = match case_id {
            Some(id) => crate::authz::case_access(&mut tx, &a, id).await?.is_staff(),
            None => true,
        };
        return Err(AppError::conflict(if visible {
            format!("Maintenance conflicts with {label}.")
        } else {
            "Maintenance conflicts with an occupied resource.".into()
        }));
    }
    let id:i64=sqlx::query_scalar("INSERT INTO occupancies(resource_id,source,label,start_at,end_at,created_at) VALUES (?,'maintenance',?,?,?,?) RETURNING id").bind(resource).bind(&v.label).bind(&start).bind(&end).bind(time::now_str()).fetch_one(&mut *tx).await?;
    crate::audit::record(
        &mut tx,
        a.db_id(),
        "maintenance.created",
        "occupancy",
        Some(id),
        json!({"resource":v.resource_code,"start":start,"end":end,"label":v.label}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
#[derive(Serialize, sqlx::FromRow)]
struct Road {
    id: i64,
    category: String,
    lat: f64,
    lng: f64,
    status: String,
    reported_on: String,
}
pub(super) async fn public_roads(tx: &mut sqlx::SqliteConnection) -> AppResult<Value> {
    let rows:Vec<Road>=sqlx::query_as("SELECT c.id,s.category,c.location_lat AS lat,c.location_lng AS lng,c.status,COALESCE(c.submitted_at,c.created_at) AS reported_on FROM cases c JOIN services s ON s.id=c.service_id WHERE c.module='road_issue' AND c.public_map=1 AND c.confidential=0 AND c.status<>'draft' AND c.location_lat IS NOT NULL AND c.location_lng IS NOT NULL ORDER BY c.created_at DESC").fetch_all(&mut *tx).await?;
    Ok(json!(rows.into_iter().map(|r|json!({"id":r.id,"category":r.category,"location":{"lat":r.lat,"lng":r.lng},"status_text":match r.status.as_str(){"completed"=>"Work completed","closed_duplicate"=>"Linked to an existing report","cancelled"|"withdrawn"=>"Report closed","refused"=>"Response issued",_=>"Reported — Council is reviewing"},"reported_on":time::local_date(time::parse(&r.reported_on).unwrap_or_else(|_|chrono::Utc::now())).to_string()})).collect::<Vec<_>>()))
}
pub async fn road_issues(State(st): State<AppState>) -> AppResult<Json<Value>> {
    let mut tx = st.db.acquire().await?;
    Ok(Json(public_roads(&mut tx).await?))
}
pub async fn road_response(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path(id): Path<i64>,
    Json(v): Json<Value>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&st.db).await?;
    let c = model::manage(&mut tx, &a, id, &[Role::Intake, Role::Specialist, Role::Manager]).await?;
    if c.module != "road_issue" {
        return Err(AppError::not_found());
    }
    let body = model::text(&v, "body", 12000)?;
    let expected = v["expected_revision"]
        .as_i64()
        .ok_or_else(|| AppError::field("expected_revision", "Reload the case before issuing a response."))?;
    crate::cases::core::bump_revision(&mut tx, id, Some(expected)).await?;
    let document =
        crate::documents::api::issue_letter(&mut tx, &st, &a, id, "road_response", "Road issue response", body).await?;
    model::record(
        &mut tx,
        a.db_id(),
        id,
        "road.response_issued",
        "Road issue response letter issued.",
        json!({"document_id":document}),
    )
    .await?;
    model::applicant_notice(
        &mut tx,
        &c,
        "Council's road issue response is ready",
        "Open your request to read the response letter.",
    )
    .await?;
    crate::cases::workflow::try_auto_advance(&mut tx, &st, id).await?;
    tx.commit().await?;
    Ok(Json(json!({"document_id":document})))
}

pub async fn road_detail(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let mut tx = st.db.acquire().await?;
    let (c, access) = authz::require_staff_case(&mut tx, &a, id).await?;
    if c.module != "road_issue" {
        return Err(AppError::not_found());
    }
    let can_issue = access.can_manage()
        && a.roles_for_service(c.service_id)
            .iter()
            .any(|r| [Role::Intake, Role::Specialist, Role::Manager].contains(r))
        && crate::documents::api::letter_due(&mut tx, &c, "road_response").await?;
    let issued: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM issued_letters WHERE case_id=? AND letter_type='road_response')",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    Ok(Json(json!({"revision":c.revision,"can_issue":can_issue,"issued":issued,"location":c.location_text})))
}
