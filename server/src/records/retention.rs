//! Retention administration, finished-case search and preservation holds.
use super::common::{self, require_role, rows};
use crate::{
    auth::Actor,
    authz::{self, Role},
    db::{SqlValue, write_tx},
    error::{AppError, AppResult},
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
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/retention-rules", get(rules).post(save_rule))
        .route("/api/records/search", get(search))
        .route("/api/records/disposal-candidates", get(candidates))
        .route("/api/records/legal-holds", get(holds))
        .route("/api/records/cases/{id}", get(panel))
        .route("/api/records/cases/{id}/legal-hold", post(place_hold))
        .route("/api/records/cases/{id}/legal-hold/release", post(release_hold))
        .route("/api/records/cases/{id}/dispose", post(dispose))
}
async fn rules(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut conn = state.db.acquire().await?;
    Ok(Json(json!(rows(&mut conn, "SELECT * FROM retention_rules ORDER BY record_class", &[]).await?)))
}
#[derive(Deserialize)]
struct Rule {
    record_class: String,
    retain_years: i32,
    description: String,
}
async fn save_rule(State(state): State<AppState>, actor: Actor, Json(body): Json<Rule>) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    if !["default", "building", "hire", "complaint", "works"].contains(&body.record_class.as_str()) {
        return Err(AppError::field("record_class", "Choose a recognised record class."));
    }
    if !(0..=100).contains(&body.retain_years) {
        return Err(AppError::field("retain_years", "Enter whole years between 0 and 100."));
    }
    let description = common::text(&body.description, "description", 1000)?;
    let mut tx = write_tx(&state.db).await?;
    sqlx::query("INSERT INTO retention_rules(record_class,retain_years,trigger_event,description) VALUES(?,?,'case_closed',?) ON CONFLICT(record_class) DO UPDATE SET retain_years=excluded.retain_years,description=excluded.description")
        .bind(&body.record_class).bind(body.retain_years).bind(description).execute(&mut *tx).await?;
    common::admin_audit(
        &mut tx,
        &actor,
        "records.retention_rule",
        "retention_rule",
        None,
        json!({"record_class":body.record_class,"retain_years":body.retain_years}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Default, Deserialize)]
struct Search {
    person: Option<String>,
    property: Option<String>,
    number: Option<String>,
}
fn like(value: &str) -> String {
    format!("%{}%", value.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"))
}
async fn search(State(state): State<AppState>, actor: Actor, Query(f): Query<Search>) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Manager)?;
    let scope = authz::case_scope_sql(&actor);
    let mut binds = scope.binds;
    let mut predicate = format!("{} AND c.closed_at IS NOT NULL", scope.sql);
    for (column, value) in [("c.applicant_name", f.person), ("c.property_ref", f.property), ("c.number", f.number)] {
        if let Some(v) = value.filter(|s| !s.trim().is_empty()) {
            predicate.push_str(&format!(" AND {column} LIKE ? ESCAPE '\\'"));
            binds.push(SqlValue::Text(like(v.trim())));
        }
    }
    let mut conn = state.db.acquire().await?;
    Ok(Json(json!(rows(&mut conn,&format!("SELECT c.id,c.number,c.title,c.status,c.applicant_name,c.property_ref,c.retention_until,c.legal_hold,c.closed_at FROM cases c WHERE {predicate} ORDER BY c.closed_at DESC"),&binds).await?)))
}
async fn candidates(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Manager)?;
    let scope = authz::case_scope_sql(&actor);
    let mut binds = scope.binds;
    binds.push(SqlValue::Text(time::fmt_date(time::local_date(state.now()))));
    let mut conn = state.db.acquire().await?;
    Ok(Json(json!(rows(&mut conn,&format!("SELECT c.id,c.number,c.title,c.retention_until FROM cases c WHERE {} AND c.closed_at IS NOT NULL AND c.legal_hold=0 AND c.retention_until<? AND NOT EXISTS(SELECT 1 FROM legal_holds h WHERE h.case_id=c.id AND h.released_at IS NULL) AND NOT EXISTS(SELECT 1 FROM disposal_events e WHERE e.case_id=c.id) ORDER BY c.retention_until",scope.sql),&binds).await?)))
}
async fn holds(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Manager)?;
    let scope = authz::case_scope_sql(&actor);
    let mut conn = state.db.acquire().await?;
    Ok(Json(json!(rows(&mut conn,&format!("SELECT c.id,c.number,c.title,h.reason,h.placed_at FROM cases c JOIN legal_holds h ON h.case_id=c.id WHERE {} AND h.released_at IS NULL ORDER BY h.placed_at DESC",scope.sql),&scope.binds).await?)))
}
async fn panel(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    let (case, _) = authz::require_staff_case(&mut conn, &actor, id).await?;
    Ok(Json(
        json!({"legal_hold":case.legal_hold==1,"revision":case.revision,"retention_until":case.retention_until,"holds":rows(&mut conn,"SELECT reason,placed_at,released_at FROM legal_holds WHERE case_id=? ORDER BY id",&[SqlValue::Int(id)]).await?,"disposed":rows(&mut conn,"SELECT reason,at FROM disposal_events WHERE case_id=? ORDER BY id",&[SqlValue::Int(id)]).await?}),
    ))
}
#[derive(Deserialize)]
pub struct Reason {
    pub reason: String,
    pub expected_revision: Option<i64>,
}
async fn place_hold(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(body): Json<Reason>,
) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Manager)?;
    let reason = common::text(&body.reason, "reason", 2000)?;
    let mut tx = write_tx(&state.db).await?;
    let (case, _) = authz::require_staff_case(&mut tx, &actor, id).await?;
    if case.legal_hold == 1 {
        return Err(AppError::conflict("This case is already on legal hold."));
    }
    common::check_revision(&case, body.expected_revision)?;
    sqlx::query("INSERT INTO legal_holds(case_id,reason,placed_by,placed_at) VALUES(?,?,?,?)")
        .bind(id)
        .bind(&reason)
        .bind(actor.user_id)
        .bind(time::fmt(state.now()))
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE cases SET legal_hold=1 WHERE id=?").bind(id).execute(&mut *tx).await?;
    common::changed(
        &mut tx,
        &actor,
        id,
        "records.legal_hold",
        "Documents preserved under a legal hold.",
        json!({"reason":reason}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn release_hold(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(body): Json<Reason>,
) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Manager)?;
    let reason = common::text(&body.reason, "reason", 2000)?;
    let mut tx = write_tx(&state.db).await?;
    let (case, _) = authz::require_staff_case(&mut tx, &actor, id).await?;
    common::check_revision(&case, body.expected_revision)?;
    let count =
        sqlx::query("UPDATE legal_holds SET released_at=?,released_by=? WHERE case_id=? AND released_at IS NULL")
            .bind(time::fmt(state.now()))
            .bind(actor.user_id)
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if count == 0 {
        return Err(AppError::conflict("There is no active hold to release."));
    }
    sqlx::query("UPDATE cases SET legal_hold=0 WHERE id=?").bind(id).execute(&mut *tx).await?;
    common::changed(
        &mut tx,
        &actor,
        id,
        "records.legal_hold_release",
        "Legal hold released.",
        json!({"reason":reason}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn dispose(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(body): Json<Reason>,
) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Manager)?;
    let reason = common::text(&body.reason, "reason", 2000)?;
    let mut tx = write_tx(&state.db).await?;
    let (case, _) = authz::require_staff_case(&mut tx, &actor, id).await?;
    let held: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM legal_holds WHERE case_id=? AND released_at IS NULL)")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    if case.legal_hold == 1 || held {
        return Err(AppError::conflict("A legal hold prevents disposal."));
    }
    let today = time::fmt_date(time::local_date(state.now()));
    if case.closed_at.is_none() || case.retention_until.as_ref().is_none_or(|until| until >= &today) {
        return Err(AppError::conflict("The case has not reached its disposal date."));
    }
    common::check_revision(&case, body.expected_revision)?;
    let paths = super::contracts::dispose_documents(&mut tx, id).await?;
    sqlx::query("INSERT INTO disposal_events(case_id,actor_user_id,reason,at) VALUES(?,?,?,?)")
        .bind(id)
        .bind(actor.user_id)
        .bind(&reason)
        .bind(time::fmt(state.now()))
        .execute(&mut *tx)
        .await?;
    common::changed(
        &mut tx,
        &actor,
        id,
        "records.dispose",
        "Document files disposed of; case history and decisions retained.",
        json!({"reason":reason}),
    )
    .await?;
    tx.commit().await?;
    for hash in paths {
        let path = crate::storage::blob_path(&state.cfg.blobs_dir(), &hash);
        if let Err(err) = std::fs::remove_file(path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(error=%err,"disposed blob cleanup deferred");
        }
    }
    Ok(Json(json!({"ok":true})))
}
