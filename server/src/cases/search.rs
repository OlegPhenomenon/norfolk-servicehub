use super::core::CaseRow;
use crate::{
    auth::{Actor, StaffActor},
    authz,
    db::{self, SqlValue},
    error::{AppError, AppResult},
    services::definition::{self, ServiceDefinition},
    state::AppState,
    web::{Json, Query},
};
use axum::{Router, extract::State, routing::get};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
pub fn routes() -> Router<AppState> {
    Router::new().route("/api/my/cases", get(my_cases)).route("/api/staff/cases", get(staff_cases))
}
pub fn status_text(c: &CaseRow, def: &ServiceDefinition) -> String {
    match c.status.as_str() {
        "draft" => "Draft — ready to continue".into(),
        "waiting_on_applicant" => "Your reply is needed".into(),
        "completed" => "Completed".into(),
        "refused" => "Not approved".into(),
        "withdrawn" => "Withdrawn".into(),
        "cancelled" => "Cancelled".into(),
        "closed_duplicate" => "Linked to the original request".into(),
        _ => def
            .step(c.current_step.as_deref().unwrap_or(""))
            .map(|s| s.applicant_label.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or("We are reviewing your request".into()),
    }
}
pub async fn summary(tx: &mut SqliteConnection, c: &CaseRow) -> AppResult<Value> {
    let name: String =
        sqlx::query_scalar("SELECT name FROM services WHERE id=?").bind(c.service_id).fetch_one(&mut *tx).await?;
    let def = definition::load_for_case(tx, c).await?;
    let mut value = json!({"id":c.id,"number":c.number,"service_id":c.service_id,"service_name":name,"module":c.module,"title":c.title,"status":c.status,"current_step":c.current_step,"applicant_name":c.applicant_name,"revision":c.revision,"confidential":c.is_confidential(),"created_at":c.created_at,"submitted_at":c.submitted_at,"updated_at":c.updated_at,"applicant_status_text":status_text(c,&def)});
    let required:Option<(i64,String)>=sqlx::query_as("SELECT id,body FROM case_messages WHERE case_id=? AND requires_response=1 AND resolved_at IS NULL ORDER BY id LIMIT 1").bind(c.id).fetch_optional(&mut *tx).await?;
    value["required_action"] = required.map(|(id, body)| json!({"message_id":id,"body":body})).unwrap_or(Value::Null);
    Ok(value)
}
#[derive(Default, Deserialize)]
struct Filter {
    queue: Option<String>,
    service: Option<String>,
    status: Option<String>,
    q: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}
async fn my_cases(State(state): State<AppState>, actor: Actor, Query(filter): Query<Filter>) -> AppResult<Json<Value>> {
    list(&state, &actor, filter, false).await
}
async fn staff_cases(
    State(state): State<AppState>,
    StaffActor(actor): StaffActor,
    Query(filter): Query<Filter>,
) -> AppResult<Json<Value>> {
    list(&state, &actor, filter, true).await
}
async fn list(state: &AppState, actor: &Actor, filter: Filter, staff: bool) -> AppResult<Json<Value>> {
    let mut scope = authz::case_scope_sql(actor);
    // Applicants' never-submitted drafts (also deleted ones) are outside the staff scope (authz).
    if !staff {
        // Staff roles must never turn /my into a staff listing.
        scope.sql.push_str(" AND (c.status='draft' OR c.submitted_at IS NOT NULL) AND ((c.applicant_org_id IS NULL AND c.applicant_user_id=?) OR EXISTS(SELECT 1 FROM memberships m WHERE m.organisation_id=c.applicant_org_id AND m.user_id=? AND m.status='active') OR EXISTS(SELECT 1 FROM case_representatives r WHERE r.case_id=c.id AND r.user_id=? AND r.status='active'))");
        scope.binds.extend([SqlValue::Int(actor.user_id), SqlValue::Int(actor.user_id), SqlValue::Int(actor.user_id)]);
    }
    if let Some(queue) = filter.queue.as_deref() {
        match queue {
            "new" => scope.sql.push_str(" AND c.status='submitted'"),
            "waiting" => scope.sql.push_str(" AND c.status='waiting_on_applicant'"),
            "overdue" => scope
                .sql
                .push_str(" AND EXISTS(SELECT 1 FROM deadlines d WHERE d.case_id=c.id AND d.status='breached')"),
            "mine" => {
                scope.sql.push_str(" AND EXISTS(SELECT 1 FROM case_assignments a WHERE a.case_id=c.id AND a.user_id=? AND a.ended_at IS NULL)");
                scope.binds.push(SqlValue::Int(actor.user_id));
            }
            "all" => {}
            _ => return Err(AppError::field("queue", "Choose a valid queue.")),
        }
    }
    if let Some(service) = filter.service.filter(|s| !s.is_empty()) {
        scope.sql.push_str(" AND c.service_id IN(SELECT id FROM services WHERE slug=?)");
        scope.binds.push(SqlValue::Text(service));
    }
    if let Some(status) = filter.status.filter(|s| !s.is_empty()) {
        scope.sql.push_str(" AND c.status=?");
        scope.binds.push(SqlValue::Text(status));
    }
    if let Some(q) = filter.q.filter(|s| !s.trim().is_empty()) {
        let fts = crate::services::catalog::prefix_query(&q, false);
        if !fts.is_empty() {
            scope
                .sql
                .push_str(" AND (c.number=? OR c.id IN(SELECT case_id FROM case_search WHERE case_search MATCH ?))");
            scope.binds.extend([SqlValue::Text(q.trim().into()), SqlValue::Text(fts)]);
        }
    }
    let page = filter.page.unwrap_or(1).max(1);
    let size = filter.page_size.unwrap_or(30).clamp(1, 100);
    let total_sql = format!("SELECT COUNT(*) FROM cases c WHERE {}", scope.sql);
    let total: i64 = db::bind_all_scalar(sqlx::query_scalar(&total_sql), &scope.binds).fetch_one(&state.db).await?;
    let sql = format!(
        "SELECT c.* FROM cases c WHERE {} ORDER BY CASE WHEN c.status='waiting_on_applicant' THEN 0 ELSE 1 END,c.updated_at DESC LIMIT ? OFFSET ?",
        scope.sql
    );
    scope.binds.extend([SqlValue::Int(size), SqlValue::Int((page - 1) * size)]);
    let rows: Vec<CaseRow> = db::bind_all_as(sqlx::query_as(&sql), &scope.binds).fetch_all(&state.db).await?;
    let mut tx = state.db.acquire().await?;
    let mut items = vec![];
    for c in rows {
        items.push(summary(&mut tx, &c).await?);
    }
    Ok(Json(json!({"items":items,"total":total,"page":page,"page_size":size})))
}
