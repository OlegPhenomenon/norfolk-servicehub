use super::{
    core::{self, Visibility},
    record,
};
use crate::{
    auth::{Actor, StaffActor},
    authz, db,
    error::{AppError, AppResult},
    notify::{self, Notice},
    state::AppState,
    time,
    web::{Json, Path, Query},
};
use axum::{
    Router,
    extract::State,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cases/{id}/assign", post(assign))
        .route("/api/cases/{id}/assignments/{aid}/end", post(end))
        .route("/api/cases/{id}/assignments", get(list))
        .route("/api/cases/{id}/escalate", post(escalate))
        .route("/api/staff/users", get(users))
}
#[derive(Deserialize)]
struct Assign {
    user_id: i64,
    role: String,
    reason: String,
    expected_revision: i64,
    #[serde(default)]
    replace_owner: bool,
}
#[derive(Deserialize)]
struct Reason {
    reason: String,
    expected_revision: i64,
}
pub async fn history(tx: &mut SqliteConnection, id: i64) -> AppResult<Value> {
    let rows:Vec<AssignmentHistoryRow>=sqlx::query_as("SELECT a.id,a.user_id,u.display_name,a.role,a.reason,a.assigned_at,a.ended_at,a.ended_reason,by.display_name FROM case_assignments a JOIN users u ON u.id=a.user_id LEFT JOIN users by ON by.id=a.assigned_by WHERE case_id=? ORDER BY a.id").bind(id).fetch_all(&mut *tx).await?;
    Ok(json!(rows.into_iter().map(|(id,user_id,name,role,reason,assigned_at,ended_at,ended_reason,assigned_by)|json!({"id":id,"user_id":user_id,"name":name,"role":role,"reason":reason,"assigned_at":assigned_at,"ended_at":ended_at,"ended_reason":ended_reason,"assigned_by":assigned_by})).collect::<Vec<_>>()))
}
pub(crate) async fn add(
    tx: &mut SqliteConnection,
    actor: &Actor,
    id: i64,
    user_id: i64,
    role: &str,
    reason: &str,
    replace: bool,
) -> AppResult<()> {
    if reason.trim().is_empty() {
        return Err(AppError::field("reason", "Record the reason for the assignment."));
    }
    if !["owner", "collaborator"].contains(&role) {
        return Err(AppError::field("role", "Choose owner or collaborator."));
    }
    let recipient = Actor::load(tx, user_id, true)
        .await
        .map_err(|_| AppError::field("user_id", "Choose an active staff member."))?;
    if !authz::case_access(tx, &recipient, id).await?.can_manage() {
        return Err(AppError::field("user_id", "This officer cannot manage this request."));
    }
    if replace && role == "owner" {
        sqlx::query("UPDATE case_assignments SET ended_at=?,ended_reason=? WHERE case_id=? AND role='owner' AND ended_at IS NULL").bind(time::now_str()).bind(reason).bind(id).execute(&mut *tx).await?;
        record(
            tx,
            actor,
            id,
            "assignment.ended",
            Visibility::Staff,
            "Previous owner's assignment ended.",
            json!({"reason":reason}),
        )
        .await?;
    }
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM case_assignments WHERE case_id=? AND user_id=? AND role=? AND ended_at IS NULL)",
    )
    .bind(id)
    .bind(user_id)
    .bind(role)
    .fetch_one(&mut *tx)
    .await?;
    if exists {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO case_assignments(case_id,user_id,role,assigned_by,reason,assigned_at) VALUES (?,?,?,?,?,?)",
    )
    .bind(id)
    .bind(user_id)
    .bind(role)
    .bind(actor.db_id())
    .bind(reason)
    .bind(time::now_str())
    .execute(&mut *tx)
    .await?;
    record(
        tx,
        actor,
        id,
        "assignment.created",
        Visibility::Staff,
        &format!("{} assigned as {role}.", recipient.display_name),
        json!({"user_id":user_id,"reason":reason}),
    )
    .await?;
    notify::send(
        tx,
        Notice {
            user_id: Some(user_id),
            case_id: Some(id),
            subject: "Request assigned to you".into(),
            body: reason.into(),
            link: Some(format!("/staff/cases/{id}")),
            ..Notice::default()
        },
    )
    .await
}
async fn assign(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<Assign>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&state.db).await?;
    let (_, access) = authz::require_staff_case(&mut tx, &actor, id).await?;
    if !access.can_manage() {
        return Err(AppError::forbidden());
    }
    add(&mut tx, &actor, id, input.user_id, &input.role, &input.reason, input.replace_owner).await?;
    core::bump_revision(&mut tx, id, Some(input.expected_revision)).await?;
    let result = history(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(result))
}
async fn end(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, aid)): Path<(i64, i64)>,
    Json(input): Json<Reason>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&state.db).await?;
    let (_, access) = authz::require_staff_case(&mut tx, &actor, id).await?;
    if !access.can_manage() {
        return Err(AppError::forbidden());
    }
    if input.reason.trim().is_empty() {
        return Err(AppError::field("reason", "Record why this assignment is ending."));
    }
    core::bump_revision(&mut tx, id, Some(input.expected_revision)).await?;
    let rows = sqlx::query(
        "UPDATE case_assignments SET ended_at=?,ended_reason=? WHERE id=? AND case_id=? AND ended_at IS NULL",
    )
    .bind(time::fmt(state.now()))
    .bind(&input.reason)
    .bind(aid)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if rows == 0 {
        return Err(AppError::not_found());
    }
    record(
        &mut tx,
        &actor,
        id,
        "assignment.ended",
        Visibility::Staff,
        "Officer assignment ended.",
        json!({"assignment_id":aid,"reason":input.reason}),
    )
    .await?;
    let result = history(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(result))
}
async fn escalate(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<Reason>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&state.db).await?;
    let (case, access) = authz::require_staff_case(&mut tx, &actor, id).await?;
    if !access.can_manage() {
        return Err(AppError::forbidden());
    }
    let candidates:Vec<i64>=sqlx::query_scalar("SELECT DISTINCT u.id FROM users u JOIN role_grants r ON r.user_id=u.id WHERE u.is_active=1 AND r.role='manager' AND r.revoked_at IS NULL AND (r.scope_service_id IS NULL OR r.scope_service_id=?) ORDER BY u.id").bind(case.service_id).fetch_all(&mut *tx).await?;
    let mut manager = None;
    for uid in candidates {
        let a = Actor::load(&mut tx, uid, true).await?;
        if authz::case_access(&mut tx, &a, id).await?.can_manage() {
            manager = Some(uid);
            break;
        }
    }
    let manager = manager.ok_or_else(|| AppError::conflict("No eligible manager is available."))?;
    add(&mut tx, &actor, id, manager, "collaborator", &input.reason, false).await?;
    core::bump_revision(&mut tx, id, Some(input.expected_revision)).await?;
    record(
        &mut tx,
        &actor,
        id,
        "case.escalated",
        Visibility::Staff,
        "Request escalated to the manager.",
        json!({"reason":input.reason,"manager":manager}),
    )
    .await?;
    let result = history(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(result))
}
async fn list(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut tx = state.db.acquire().await?;
    authz::require_staff_case(&mut tx, &actor, id).await?;
    Ok(Json(history(&mut tx, id).await?))
}
#[derive(Deserialize)]
struct UserFilter {
    role: Option<String>,
}
async fn users(
    State(state): State<AppState>,
    StaffActor(_actor): StaffActor,
    Query(filter): Query<UserFilter>,
) -> AppResult<Json<Value>> {
    let rows:Vec<(i64,String,String)>=sqlx::query_as("SELECT DISTINCT u.id,u.display_name,u.email FROM users u WHERE u.kind='staff' AND u.is_active=1 AND (? IS NULL OR EXISTS(SELECT 1 FROM role_grants r WHERE r.user_id=u.id AND r.role=? AND r.revoked_at IS NULL)) ORDER BY u.display_name").bind(&filter.role).bind(&filter.role).fetch_all(&state.db).await?;
    Ok(Json(
        json!({"items":rows.into_iter().map(|(id,name,email)|json!({"id":id,"name":name,"email":email})).collect::<Vec<_>>()}),
    ))
}

type AssignmentHistoryRow =
    (i64, i64, String, String, Option<String>, String, Option<String>, Option<String>, Option<String>);
