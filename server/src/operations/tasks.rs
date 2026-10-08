use super::model;
use crate::{
    auth::{Actor, StaffActor},
    authz::{self, CaseAccess, Role},
    db,
    error::{AppError, AppResult},
    idempotency,
    notify::{self, Notice},
    state::AppState,
    storage, time,
    web::{Json, Path},
};
use axum::{
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;

/// Deliberately no CaseRow, applicant details, documents or internal case notes.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Task {
    pub id: i64,
    pub case_id: i64,
    pub kind: String,
    pub title: String,
    pub instructions: String,
    pub assigned_to: Option<i64>,
    pub scheduled_start: Option<String>,
    pub scheduled_end: Option<String>,
    pub location_text: Option<String>,
    pub location_lat: Option<f64>,
    pub location_lng: Option<f64>,
    pub checklist_json: String,
    pub status: String,
    pub result_text: Option<String>,
    pub revision: i64,
    pub step_run_id: Option<i64>,
}
/// Staff roles that assign and cancel field tasks (they never record the field result).
const TASK_MANAGERS: &[Role] = &[Role::Intake, Role::Manager, Role::Specialist];
pub async fn load(tx: &mut SqliteConnection, id: i64) -> AppResult<Task> {
    Ok(sqlx::query_as("SELECT * FROM tasks WHERE id=?").bind(id).fetch_one(&mut *tx).await?)
}
/// Reads: the assigned field worker or any staff member with case access. Writes (notes, photos, checklist,
/// result, status, job card): only the assigned field worker; managing staff assign/cancel through their own
/// commands and see a read-only view.
pub async fn require(tx: &mut SqliteConnection, a: &Actor, id: i64, write: bool) -> AppResult<Task> {
    let t = load(tx, id).await?;
    let access = authz::case_access(tx, a, t.case_id).await?;
    let assigned =
        a.is_staff() && a.has_role(Role::FieldWorker) && t.assigned_to == Some(a.user_id) && access != CaseAccess::None;
    if assigned {
        return Ok(t);
    }
    if !access.is_staff() {
        return Err(AppError::not_found());
    }
    if write {
        return Err(AppError::forbidden_msg("Only the assigned field worker records the result of this task."));
    }
    Ok(t)
}
pub async fn notify_assignee(tx: &mut SqliteConnection, id: i64, user: Option<i64>, title: &str) -> AppResult<()> {
    let Some(uid) = user else {
        return Ok(());
    };
    let case_id: i64 = sqlx::query_scalar("SELECT case_id FROM tasks WHERE id=?").bind(id).fetch_one(&mut *tx).await?;
    notify::send(
        tx,
        Notice {
            user_id: Some(uid),
            case_id: Some(case_id),
            subject: "Field task assigned".into(),
            body: title.into(),
            link: Some(format!("/staff/field/{id}")),
            ..Notice::default()
        },
    )
    .await
}
#[derive(Serialize, sqlx::FromRow)]
struct UpdateRow {
    id: i64,
    kind: String,
    body: Option<String>,
    blob_id: Option<i64>,
    created_offline_at: Option<String>,
    created_at: String,
}
pub async fn projection(tx: &mut SqliteConnection, t: &Task) -> AppResult<Value> {
    let updates: Vec<UpdateRow> = sqlx::query_as(
        "SELECT id,kind,body,blob_id,created_offline_at,created_at FROM task_updates WHERE task_id=? ORDER BY id",
    )
    .bind(t.id)
    .fetch_all(&mut *tx)
    .await?;
    let mut v = json!(t);
    v["checklist"] = serde_json::from_str(&t.checklist_json)?;
    v.as_object_mut().expect("task object").remove("checklist_json");
    v["updates"] = json!(updates);
    Ok(v)
}
pub async fn list(State(st): State<AppState>, StaffActor(a): StaffActor) -> AppResult<Json<Value>> {
    a.require_any_role(&[Role::FieldWorker, Role::Intake, Role::Manager, Role::Specialist])?;
    let mut tx = st.db.acquire().await?;
    let rows:Vec<Task>=sqlx::query_as("SELECT * FROM tasks WHERE assigned_to=? AND status<>'cancelled' ORDER BY scheduled_start IS NULL, scheduled_start, id").bind(a.user_id).fetch_all(&mut *tx).await?;
    let mut out = Vec::new();
    for t in rows {
        if require(&mut tx, &a, t.id, false).await.is_ok() {
            out.push(projection(&mut tx, &t).await?);
        }
    }
    Ok(Json(json!(out)))
}
pub async fn detail(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let mut tx = st.db.acquire().await?;
    let t = require(&mut tx, &a, id, false).await?;
    Ok(Json(projection(&mut tx, &t).await?))
}
pub async fn case_tasks(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let mut tx = st.db.acquire().await?;
    let (case, access) = authz::require_staff_case(&mut tx, &a, id).await?;
    // Assign/cancel rights only; recording the result stays with the assigned field worker.
    let can_manage =
        access.can_manage() && a.roles_for_service(case.service_id).iter().any(|r| TASK_MANAGERS.contains(r));
    let tasks: Vec<Task> =
        sqlx::query_as("SELECT * FROM tasks WHERE case_id=? ORDER BY id").bind(id).fetch_all(&mut *tx).await?;
    let mut out = Vec::new();
    for t in tasks {
        let mut v = projection(&mut tx, &t).await?;
        v["can_manage"] = json!(can_manage);
        out.push(v);
    }
    Ok(Json(json!(out)))
}
fn command(v: &Value) -> AppResult<&str> {
    model::text(v, "client_command_id", 200)
}
pub(super) async fn replay(
    tx: &mut SqliteConnection,
    a: &Actor,
    id: i64,
    v: &Value,
    scope: &str,
) -> AppResult<Option<Value>> {
    let key = command(v)?;
    if let Some(prev) = idempotency::lookup(tx, a.user_id, scope, key, &idempotency::json_hash(v)).await? {
        let mut body = prev.body;
        body["status"] = json!("already_applied");
        return Ok(Some(body));
    }
    let existing: Option<(i64, i64)> =
        sqlx::query_as("SELECT task_id,author_user_id FROM task_updates WHERE client_command_id=?")
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?;
    if existing.is_some_and(|(task, user)| task != id || user != a.user_id) {
        return Err(AppError::idempotency_mismatch());
    }
    Ok(None)
}
pub(super) async fn store_command(
    tx: &mut SqliteConnection,
    a: &Actor,
    v: &Value,
    scope: &str,
    response: &Value,
) -> AppResult<()> {
    idempotency::store(tx, a.user_id, scope, command(v)?, &idempotency::json_hash(v), StatusCode::OK, response).await
}
pub fn check_revision(t: &Task, v: &Value) -> AppResult<()> {
    if v["expected_revision"].as_i64() != Some(t.revision) {
        let mut e = AppError::stale_revision();
        e.fields.insert("current_state".into(), json!(t).to_string());
        return Err(e);
    }
    Ok(())
}
/// Applies a validated worker command inside the caller's transaction, without workflow side effects.
pub(super) async fn apply_update(
    tx: &mut SqliteConnection,
    a: &Actor,
    t: &Task,
    v: &Value,
    blob: Option<i64>,
) -> AppResult<Value> {
    let kind = model::text(v, "kind", 20)?;
    let body = v["body"].as_str().unwrap_or("");
    if body.len() > 8000 {
        return Err(AppError::field("body", "Use up to 8000 characters."));
    }
    if t.status == "cancelled" {
        return Err(AppError::conflict("This task has been cancelled."));
    }
    if let Some(s) = v["created_offline_at"].as_str() {
        time::parse(s).map_err(|_| AppError::field("created_offline_at", "Use a valid timestamp."))?;
    }
    let mut status = t.status.clone();
    let mut result = t.result_text.clone();
    let mut checklist: Value = serde_json::from_str(&t.checklist_json)?;
    match kind {
        "note" => {
            if body.trim().is_empty() {
                return Err(AppError::field("body", "Write a note."));
            }
        }
        "photo" => {
            if blob.is_none() {
                return Err(AppError::field("photo", "Attach a photo."));
            }
        }
        "status" | "result" | "checklist" => {
            check_revision(t, v)?;
            if t.status == "done" {
                return Err(AppError::conflict("This task is already complete."));
            }
            match kind {
                "status" => {
                    if !matches!(body, "in_progress" | "done") {
                        return Err(AppError::field("body", "Choose in_progress or done."));
                    }
                    if body == "done" {
                        if result.as_deref().is_none_or(|s| s.trim().is_empty())
                            || checklist.as_array().is_none_or(|c| c.iter().any(|i| i["done"] != true))
                        {
                            return Err(AppError::conflict(
                                "Record a result and complete every checklist item before marking done.",
                            ));
                        }
                        if t.kind == "equipment_job" {
                            let recorded:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM equipment_usage u JOIN equipment_requests e ON e.id=u.equipment_request_id WHERE e.task_id=?)").bind(t.id).fetch_one(&mut *tx).await?;
                            if !recorded {
                                return Err(AppError::conflict("Record the equipment job card before marking done."));
                            }
                        }
                    }
                    status = body.into();
                }
                "result" => {
                    if body.trim().is_empty() {
                        return Err(AppError::field("body", "Record the work result."));
                    }
                    result = Some(body.into());
                }
                "checklist" => {
                    let change: Value = serde_json::from_str(body)
                        .map_err(|_| AppError::field("body", "Supply checklist key and done flag."))?;
                    let done = change["done"]
                        .as_bool()
                        .ok_or_else(|| AppError::field("body", "Supply a checklist done flag."))?;
                    let item = checklist
                        .as_array_mut()
                        .and_then(|c| c.iter_mut().find(|i| i["key"] == change["key"]))
                        .ok_or_else(|| AppError::field("body", "Unknown checklist item."))?;
                    item["done"] = json!(done);
                }
                _ => {}
            }
        }
        _ => return Err(AppError::field("kind", "Choose note, photo, checklist, status or result.")),
    }
    let update_id:i64=sqlx::query_scalar("INSERT INTO task_updates(task_id,client_command_id,author_user_id,kind,body,blob_id,created_offline_at,created_at) VALUES (?,?,?,?,?,?,?,?) RETURNING id")
        .bind(t.id).bind(command(v)?).bind(a.user_id).bind(kind).bind(body).bind(blob).bind(v["created_offline_at"].as_str()).bind(time::now_str()).fetch_one(&mut *tx).await?;
    sqlx::query("UPDATE tasks SET status=?,result_text=?,checklist_json=?,revision=revision+1,completed_at=CASE WHEN ?='done' THEN ? ELSE completed_at END WHERE id=?")
        .bind(&status).bind(result).bind(checklist.to_string()).bind(&status).bind(time::now_str()).bind(t.id).execute(&mut *tx).await?;
    model::record(
        tx,
        a.db_id(),
        t.case_id,
        "task.updated",
        &format!("{}: {}.", t.title, if status == "done" { "work completed" } else { kind }),
        json!({"task_id":t.id,"update_id":update_id,"kind":kind}),
    )
    .await?;
    let updated = load(tx, t.id).await?;
    Ok(json!({"status":"applied","update_id":update_id,"task":projection(tx,&updated).await?}))
}
pub async fn update(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path(id): Path<i64>,
    Json(v): Json<Value>,
) -> AppResult<Json<Value>> {
    // Validate access before staging bytes; do not open a second DB connection during the write tx.
    {
        let mut conn = st.db.acquire().await?;
        require(&mut conn, &a, id, true).await?;
    }
    let staged = if let Some(p) = v["photo"].as_object() {
        let name = p.get("name").and_then(Value::as_str).unwrap_or("photo.jpg");
        let data =
            p.get("base64").and_then(Value::as_str).ok_or_else(|| AppError::field("photo", "Attach a photo."))?;
        if data.len() > 14_000_000 {
            return Err(AppError::field("photo", "Photo is larger than 10 MB."));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| AppError::field("photo", "Invalid photo encoding."))?;
        Some(storage::stage(&st, &bytes, name, storage::AllowList::Image).await?)
    } else {
        None
    };
    let mut tx = db::write_tx(&st.db).await?;
    let t = require(&mut tx, &a, id, true).await?;
    let scope = format!("operations.task.{id}");
    if let Some(prev) = replay(&mut tx, &a, id, &v, &scope).await? {
        return Ok(Json(prev));
    }
    let blob = match staged {
        Some(s) => Some(storage::register(&mut tx, s, a.db_id()).await?.id),
        None => None,
    };
    let response = apply_update(&mut tx, &a, &t, &v, blob).await?;
    if response["task"]["status"] == "done" {
        crate::cases::workflow::try_auto_advance(&mut tx, &st, t.case_id).await?;
    }
    store_command(&mut tx, &a, &v, &scope, &response).await?;
    tx.commit().await?;
    Ok(Json(response))
}
#[derive(Deserialize)]
pub struct Assign {
    assigned_to: i64,
    expected_revision: i64,
}
#[derive(Deserialize)]
pub struct Cancel {
    reason: String,
    expected_revision: i64,
}
pub(super) async fn eligible_worker(tx: &mut SqliteConnection, user: i64, case: i64) -> AppResult<()> {
    let c = crate::cases::core::load_case(tx, case).await?;
    let a = Actor::load(tx, user, true)
        .await
        .map_err(|_| AppError::field("assigned_to", "Choose an active field worker."))?;
    let denied: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM case_access_denials WHERE case_id=? AND user_id=?)")
            .bind(case)
            .bind(user)
            .fetch_one(&mut *tx)
            .await?;
    if c.is_confidential() || denied || !a.is_staff() || !a.roles_for_service(c.service_id).contains(&Role::FieldWorker)
    {
        return Err(AppError::field("assigned_to", "Choose a field worker permitted to work on this request."));
    }
    Ok(())
}
pub async fn assign(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path(id): Path<i64>,
    Json(v): Json<Assign>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&st.db).await?;
    let t = load(&mut tx, id).await?;
    model::manage(&mut tx, &a, t.case_id, TASK_MANAGERS).await?;
    check_revision(&t, &json!({"expected_revision":v.expected_revision}))?;
    eligible_worker(&mut tx, v.assigned_to, t.case_id).await?;
    if matches!(t.status.as_str(), "done" | "cancelled") {
        return Err(AppError::conflict("This task is closed."));
    }
    sqlx::query("UPDATE tasks SET assigned_to=?,revision=revision+1 WHERE id=?")
        .bind(v.assigned_to)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    model::record(
        &mut tx,
        a.db_id(),
        t.case_id,
        "task.assigned",
        &format!("Field task reassigned: {}.", t.title),
        json!({"task_id":id,"assigned_to":v.assigned_to}),
    )
    .await?;
    notify_assignee(&mut tx, id, Some(v.assigned_to), &t.title).await?;
    tx.commit().await?;
    Ok(Json(json!({"status":"assigned"})))
}
pub async fn cancel(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path(id): Path<i64>,
    Json(v): Json<Cancel>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&st.db).await?;
    let t = load(&mut tx, id).await?;
    model::manage(&mut tx, &a, t.case_id, TASK_MANAGERS).await?;
    check_revision(&t, &json!({"expected_revision":v.expected_revision}))?;
    model::text(&json!({"reason":v.reason}), "reason", 2000)?;
    if matches!(t.status.as_str(), "done" | "cancelled") {
        return Err(AppError::conflict("This task is already closed."));
    }
    sqlx::query(
        "UPDATE tasks SET status='cancelled',revision=revision+1 WHERE id=? AND status NOT IN ('done','cancelled')",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    model::record(
        &mut tx,
        a.db_id(),
        t.case_id,
        "task.cancelled",
        &format!("Field task cancelled: {}. {}", t.title, v.reason),
        json!({"task_id":id}),
    )
    .await?;
    notify_assignee(&mut tx, id, t.assigned_to, &format!("Cancelled: {}", t.title)).await?;
    tx.commit().await?;
    Ok(Json(json!({"status":"cancelled"})))
}
pub async fn refresh_venue_tasks(
    tx: &mut SqliteConnection,
    c: &crate::cases::core::CaseRow,
    actor: Option<i64>,
) -> AppResult<()> {
    let tasks:Vec<Task>=sqlx::query_as("SELECT * FROM tasks WHERE case_id=? AND kind IN ('venue_prep','venue_inspection') AND status IN ('open','in_progress')").bind(c.id).fetch_all(&mut *tx).await?;
    for t in tasks {
        let (title, instructions, start, end, location, _, _) = super::api::task_spec(tx, c, &t.kind).await?;
        sqlx::query("UPDATE tasks SET title=?,instructions=?,scheduled_start=?,scheduled_end=?,location_text=?,revision=revision+1 WHERE id=?").bind(&title).bind(instructions).bind(start).bind(end).bind(location).bind(t.id).execute(&mut *tx).await?;
        model::record(
            tx,
            actor,
            c.id,
            "task.rescheduled",
            "Field task updated for the changed booking.",
            json!({"task_id":t.id}),
        )
        .await?;
        notify_assignee(tx, t.id, t.assigned_to, &format!("Time changed: {title}")).await?;
    }
    Ok(())
}
pub async fn cancel_venue_tasks(
    tx: &mut SqliteConnection,
    c: &crate::cases::core::CaseRow,
    actor: Option<i64>,
    reason: &str,
) -> AppResult<()> {
    let tasks:Vec<Task>=sqlx::query_as("SELECT * FROM tasks WHERE case_id=? AND kind IN ('venue_prep','venue_inspection') AND status IN ('open','in_progress')").bind(c.id).fetch_all(&mut *tx).await?;
    for t in tasks {
        sqlx::query("UPDATE tasks SET status='cancelled',revision=revision+1 WHERE id=?")
            .bind(t.id)
            .execute(&mut *tx)
            .await?;
        model::record(
            tx,
            actor,
            c.id,
            "task.cancelled",
            &format!("Field task cancelled with booking: {reason}"),
            json!({"task_id":t.id}),
        )
        .await?;
        notify_assignee(tx, t.id, t.assigned_to, &format!("Cancelled: {}", t.title)).await?;
    }
    Ok(())
}
pub async fn photo(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path((id, update_id)): Path<(i64, i64)>,
) -> AppResult<impl IntoResponse> {
    let blob = {
        let mut tx = st.db.acquire().await?;
        require(&mut tx, &a, id, false).await?;
        sqlx::query_scalar::<_, Option<i64>>(
            "SELECT blob_id FROM task_updates WHERE id=? AND task_id=? AND kind='photo'",
        )
        .bind(update_id)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?
        .ok_or_else(AppError::not_found)?
    };
    let (row, bytes) = storage::read(&st, blob).await?;
    Ok(([(header::CONTENT_TYPE, row.mime), (header::CACHE_CONTROL, "private, no-store".into())], bytes))
}
