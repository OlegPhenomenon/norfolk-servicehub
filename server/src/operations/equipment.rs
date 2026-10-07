use super::{model, tasks};
use crate::{
    auth::{Actor, StaffActor},
    authz::{self, Role},
    db,
    error::{AppError, AppResult},
    finance,
    state::AppState,
    time,
    web::{Json, Path},
};
use axum::extract::State;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Request {
    pub id: i64,
    pub case_id: i64,
    pub description: String,
    pub requested_hours: Option<i64>,
    pub preferred_date: Option<String>,
    pub site_text: Option<String>,
    pub assigned_resource_id: Option<i64>,
    pub task_id: Option<i64>,
    pub created_at: String,
    pub operator_user_id: Option<i64>,
    pub scheduled_start: Option<String>,
    pub scheduled_end: Option<String>,
    pub status: String,
}
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Usage {
    pub id: i64,
    pub equipment_request_id: i64,
    pub resource_id: i64,
    pub operator_user_id: i64,
    pub started_at: String,
    pub ended_at: String,
    pub downtime_minutes: i64,
    pub billable_minutes: i64,
    pub expenses_cents: i64,
    pub expenses_note: Option<String>,
    pub recorded_by: i64,
    pub recorded_at: String,
    pub approved_by: Option<i64>,
    pub approved_at: Option<String>,
    pub client_command_id: Option<String>,
    pub final_invoice_id: Option<i64>,
}
#[derive(Deserialize)]
pub struct Schedule {
    resource_code: String,
    operator_user_id: i64,
    start: String,
    end: String,
    expected_revision: Option<i64>,
}
#[derive(Serialize, sqlx::FromRow)]
struct Invoice {
    id: i64,
    number: String,
    kind: String,
    total_cents: i64,
    basis_note: Option<String>,
}
async fn request(tx: &mut SqliteConnection, case: i64) -> AppResult<Request> {
    Ok(sqlx::query_as("SELECT * FROM equipment_requests WHERE case_id=?").bind(case).fetch_one(&mut *tx).await?)
}
pub async fn detail(State(st): State<AppState>, a: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut tx = st.db.acquire().await?;
    let (c, access) = model::read_case(&mut tx, &a, id).await?;
    let e = request(&mut tx, id).await?;
    let usage: Vec<Usage> = sqlx::query_as("SELECT * FROM equipment_usage WHERE equipment_request_id=? ORDER BY id")
        .bind(e.id)
        .fetch_all(&mut *tx)
        .await?;
    let invoices: Vec<Invoice> = sqlx::query_as(
        "SELECT id,number,kind,total_cents,basis_note FROM invoices WHERE case_id=? AND status='issued' ORDER BY id",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    Ok(Json(
        json!({"request":e,"usage":usage,"invoices":invoices,"can_schedule":access.can_manage() && a.roles_for_service(c.service_id).iter().any(|r|matches!(r,Role::Intake|Role::Specialist|Role::Manager)),"can_approve":access.is_staff() && a.roles_for_service(c.service_id).contains(&Role::Finance),"revision":c.revision,"price_note":"FY2026-27 schedule (demo copy — confirm with Council)"}),
    ))
}
pub async fn schedule(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path(id): Path<i64>,
    Json(v): Json<Schedule>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&st.db).await?;
    let c = model::manage(&mut tx, &a, id, &[Role::Intake, Role::Manager, Role::Specialist]).await?;
    if v.expected_revision.is_some_and(|r| r != c.revision) {
        return Err(AppError::stale_revision());
    }
    let e = request(&mut tx, id).await?;
    tasks::eligible_worker(&mut tx, v.operator_user_id, id).await?;
    let resource: model::Resource =
        sqlx::query_as("SELECT * FROM resources WHERE code=? AND kind='equipment' AND active=1")
            .bind(&v.resource_code)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| AppError::field("resource_code", "Choose active Council plant."))?;
    let start = time::parse(&v.start).map_err(|_| AppError::field("start", "Choose a valid start."))?;
    let end = time::parse(&v.end).map_err(|_| AppError::field("end", "Choose a valid end."))?;
    if end <= start || start <= st.now() || end - start > chrono::Duration::hours(24) {
        return Err(AppError::field("end", "Choose a future job lasting at most 24 hours."));
    }
    let usage_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM equipment_usage WHERE equipment_request_id=?)")
            .bind(e.id)
            .fetch_one(&mut *tx)
            .await?;
    if usage_exists {
        return Err(AppError::conflict("A job card is already recorded; the schedule cannot be changed."));
    }
    let busy:Option<(Option<i64>,String)>=sqlx::query_as("SELECT o.case_id, COALESCE(c.number,o.label) FROM occupancies o LEFT JOIN cases c ON c.id=o.case_id WHERE resource_id=? AND active=1 AND start_at<? AND end_at>? AND (o.case_id IS NULL OR o.case_id<>?) LIMIT 1").bind(resource.id).bind(time::fmt(end)).bind(time::fmt(start)).bind(id).fetch_optional(&mut *tx).await?;
    if let Some((conflict_case, label)) = busy {
        let label = if let Some(case) = conflict_case
            && !authz::case_access(&mut tx, &a, case).await?.is_staff()
        {
            "Unavailable".into()
        } else {
            label
        };
        return Err(AppError::conflict(format!("Plant is unavailable: {label}.")));
    }
    let operator_busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM equipment_requests WHERE status<>'cancelled' AND operator_user_id=? AND case_id<>? AND scheduled_start<? AND scheduled_end>?)").bind(v.operator_user_id).bind(id).bind(time::fmt(end)).bind(time::fmt(start)).fetch_one(&mut *tx).await?;
    if operator_busy {
        return Err(AppError::field("operator_user_id", "The operator already has a job at that time."));
    }
    sqlx::query("UPDATE occupancies SET active=0 WHERE case_id=? AND source='equipment'")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO occupancies(resource_id,source,case_id,label,start_at,end_at,created_at) VALUES (?,'equipment',?,?,?,?,?)").bind(resource.id).bind(id).bind(&c.title).bind(time::fmt(start)).bind(time::fmt(end)).bind(time::now_str()).execute(&mut *tx).await?;
    sqlx::query("UPDATE equipment_requests SET status='scheduled',assigned_resource_id=?,operator_user_id=?,scheduled_start=?,scheduled_end=? WHERE id=?").bind(resource.id).bind(v.operator_user_id).bind(time::fmt(start)).bind(time::fmt(end)).bind(e.id).execute(&mut *tx).await?;
    if let Some(task) = e.task_id {
        let t = tasks::load(&mut tx, task).await?;
        if matches!(t.status.as_str(), "done" | "cancelled") {
            return Err(AppError::conflict("The equipment task is closed."));
        }
        sqlx::query("UPDATE tasks SET assigned_to=?,scheduled_start=?,scheduled_end=?,revision=revision+1 WHERE id=?")
            .bind(v.operator_user_id)
            .bind(time::fmt(start))
            .bind(time::fmt(end))
            .bind(task)
            .execute(&mut *tx)
            .await?;
        tasks::notify_assignee(&mut tx, task, Some(v.operator_user_id), "Equipment job schedule changed").await?;
    }
    crate::cases::core::bump_revision(&mut tx, id, v.expected_revision).await?;
    let summary = format!(
        "{} scheduled for {}–{} with a Council operator.",
        resource.name,
        time::display_local(start),
        time::display_local(end)
    );
    model::record(
        &mut tx,
        a.db_id(),
        id,
        "equipment.scheduled",
        &summary,
        json!({"resource":v.resource_code,"operator":v.operator_user_id}),
    )
    .await?;
    model::applicant_notice(&mut tx, &c, "Equipment hire scheduled", &summary).await?;
    crate::cases::workflow::try_auto_advance(&mut tx, &st, id).await?;
    tx.commit().await?;
    Ok(Json(json!({"status":"scheduled"})))
}
/// Integer minutes, exact and authoritative. Reject sub-minute inputs rather than silently rounding.
pub fn billable(start: &str, end: &str, downtime: i64) -> AppResult<i64> {
    let start = time::parse(start).map_err(|_| AppError::field("started_at", "Use a valid start time."))?;
    let end = time::parse(end).map_err(|_| AppError::field("ended_at", "Use a valid end time."))?;
    let elapsed = (end - start).num_seconds();
    if elapsed <= 0
        || (end - start).num_nanoseconds().is_none_or(|n| n % 60_000_000_000 != 0)
        || elapsed > 7 * 86400
        || downtime < 0
        || downtime > elapsed / 60
    {
        return Err(AppError::field(
            "downtime_minutes",
            "Enter a positive whole-minute job duration and downtime no longer than the job.",
        ));
    }
    Ok(elapsed / 60 - downtime)
}
pub async fn usage(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path(id): Path<i64>,
    Json(v): Json<Value>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&st.db).await?;
    let t = tasks::require(&mut tx, &a, id, true).await?;
    if t.kind != "equipment_job" || t.assigned_to != Some(a.user_id) || !a.has_role(Role::FieldWorker) {
        return Err(AppError::not_found());
    }
    let scope = format!("operations.usage.{id}");
    if let Some(prev) = tasks::replay(&mut tx, &a, id, &v, &scope).await? {
        return Ok(Json(prev));
    }
    if t.status == "cancelled" || t.status == "done" {
        return Err(AppError::conflict("This task is closed."));
    }
    let e = request(&mut tx, t.case_id).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM equipment_usage WHERE equipment_request_id=?)")
        .bind(e.id)
        .fetch_one(&mut *tx)
        .await?;
    if exists {
        return Err(AppError::conflict("A job card is already recorded for this hire."));
    }
    let start = model::text(&v, "started_at", 50)?;
    let end = model::text(&v, "ended_at", 50)?;
    let downtime =
        v["downtime_minutes"].as_i64().ok_or_else(|| AppError::field("downtime_minutes", "Enter whole minutes."))?;
    let minutes = billable(start, end, downtime)?;
    if time::parse(end)? > st.now() {
        return Err(AppError::field("ended_at", "Actual usage cannot end in the future."));
    }
    let expenses = v["expenses_cents"]
        .as_i64()
        .filter(|c| (0..=100_000_000).contains(c))
        .ok_or_else(|| AppError::field("expenses_cents", "Enter non-negative agreed expenses in cents."))?;
    if expenses > 0 {
        model::text(&v, "expenses_note", 2000)?;
    }
    let uid:i64=sqlx::query_scalar("INSERT INTO equipment_usage(equipment_request_id,resource_id,operator_user_id,started_at,ended_at,downtime_minutes,billable_minutes,expenses_cents,expenses_note,recorded_by,recorded_at,client_command_id) VALUES (?,?,?,?,?,?,?,?,?,?,?,?) RETURNING id")
        .bind(e.id).bind(e.assigned_resource_id.ok_or_else(||AppError::conflict("Assign plant before recording usage."))?).bind(a.user_id).bind(time::fmt(time::parse(start)?)).bind(time::fmt(time::parse(end)?)).bind(downtime).bind(minutes).bind(expenses).bind(v["expenses_note"].as_str()).bind(a.user_id).bind(time::now_str()).bind(model::text(&v,"client_command_id",200)?).fetch_one(&mut *tx).await?;
    model::record(
        &mut tx,
        a.db_id(),
        t.case_id,
        "equipment.usage_recorded",
        &format!("Job card recorded: {} h {} min billable; {downtime} min downtime.", minutes / 60, minutes % 60),
        json!({"usage_id":uid}),
    )
    .await?;
    model::role_notice(
        &mut tx,
        t.case_id,
        "finance",
        "Approve equipment job card",
        "Actual usage is ready for review and a final invoice.",
    )
    .await?;
    let response = json!({"status":"applied","usage_id":uid,"billable_minutes":minutes});
    tasks::store_command(&mut tx, &a, &v, &scope, &response).await?;
    tx.commit().await?;
    Ok(Json(response))
}
pub async fn approve(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path((id, uid)): Path<(i64, i64)>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&st.db).await?;
    let (c, _) = authz::require_staff_case(&mut tx, &a, id).await?;
    if !a.roles_for_service(c.service_id).contains(&Role::Finance) {
        return Err(AppError::forbidden());
    }
    let e = request(&mut tx, id).await?;
    let u: Usage = sqlx::query_as("SELECT * FROM equipment_usage WHERE id=? AND equipment_request_id=?")
        .bind(uid)
        .bind(e.id)
        .fetch_one(&mut *tx)
        .await?;
    if u.approved_at.is_some() {
        return Ok(Json(json!({"status":"already_applied","invoice_id":u.final_invoice_id})));
    }
    let code: String = sqlx::query_scalar("SELECT price_item_code FROM resources WHERE id=?")
        .bind(u.resource_id)
        .fetch_one(&mut *tx)
        .await?;
    let date = time::local_date(time::parse(&u.started_at)?);
    let mut hourly =
        finance::api::quote(&mut tx, &code, u.billable_minutes * 1000 / 60, Some(u.billable_minutes), date).await?;
    hourly.calc = json!({"usage_id":uid,"started_at":u.started_at,"ended_at":u.ended_at,"downtime_minutes":u.downtime_minutes,"billable_minutes":u.billable_minutes,"requested_hours":e.requested_hours});
    let mut lines = vec![hourly];
    if u.expenses_cents > 0 {
        let mut expense = finance::api::quote(&mut tx, "EQUIP_EXPENSES", 1000, None, date).await?;
        expense.unit_amount_cents = u.expenses_cents;
        expense.amount_cents = u.expenses_cents;
        expense.description =
            format!("Agreed pass-through expenses: {}", u.expenses_note.as_deref().unwrap_or("Job card"));
        expense.calc = json!({"usage_id":uid,"expenses_cents":u.expenses_cents,"expenses_note":u.expenses_note});
        lines.push(expense);
    }
    let elapsed_minutes = (time::parse(&u.ended_at)? - time::parse(&u.started_at)?).num_minutes();
    let basis = format!(
        "Requested {} h; actual {} h {} min elapsed on job card ({}–{}, {} min downtime); billable {} h {} min",
        e.requested_hours.unwrap_or(0),
        elapsed_minutes / 60,
        elapsed_minutes % 60,
        time::to_local(time::parse(&u.started_at)?).format("%H:%M"),
        time::to_local(time::parse(&u.ended_at)?).format("%H:%M"),
        u.downtime_minutes,
        u.billable_minutes / 60,
        u.billable_minutes % 60
    );
    let invoice =
        finance::api::issue_invoice(&mut tx, &st, &a, id, "invoice", date, lines, Some(basis.clone())).await?;
    sqlx::query("UPDATE equipment_usage SET approved_by=?,approved_at=?,final_invoice_id=? WHERE id=?")
        .bind(a.user_id)
        .bind(time::now_str())
        .bind(invoice)
        .bind(uid)
        .execute(&mut *tx)
        .await?;
    model::record(
        &mut tx,
        a.db_id(),
        id,
        "equipment.usage_approved",
        &format!("Job card approved and final invoice issued. {basis}."),
        json!({"usage_id":uid,"invoice_id":invoice}),
    )
    .await?;
    model::applicant_notice(&mut tx, &c, "Your equipment hire invoice is ready", &basis).await?;
    crate::cases::workflow::try_auto_advance(&mut tx, &st, id).await?;
    tx.commit().await?;
    Ok(Json(json!({"status":"approved","invoice_id":invoice})))
}
