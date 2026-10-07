//! Durable sender outbox. Network calls never hold a database transaction.
use super::common::{self, require_role, rows};
use crate::{
    auth::Actor,
    authz::{self, Role},
    db::{SqlValue, write_tx},
    error::{AppError, AppResult},
    jobs,
    state::AppState,
    time,
    web::{Json, Path, Query},
};
use axum::{
    Router,
    extract::State,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use std::time::Duration;
const MAX_ATTEMPTS: i64 = 8;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/integrations", get(list))
        .route("/api/admin/integrations/{id}/retry", post(retry))
        .route("/api/admin/integrations/systems", get(systems))
        .route("/api/admin/integrations/systems/{code}", post(toggle))
        .route("/api/admin/mock-records/{code}", get(remote))
        .route("/api/cases/{id}/integrations", get(case_deliveries))
}
pub async fn enqueue(
    tx: &mut SqliteConnection,
    system: &str,
    case_id: Option<i64>,
    kind: &str,
    entity: i64,
    payload: Value,
) -> AppResult<()> {
    let name = match system {
        "content_manager" => "Content Manager (mock EDRMS)",
        "civica_altitude" => "Civica Altitude (mock finance)",
        _ => return Err(AppError::internal("Unknown external system")),
    };
    sqlx::query("INSERT INTO external_systems(code,name,base_url) VALUES(?,?,?) ON CONFLICT DO NOTHING")
        .bind(system)
        .bind(name)
        .bind(format!("/mock/records/{system}"))
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO mock_system_state(system_code) VALUES(?) ON CONFLICT DO NOTHING")
        .bind(system)
        .execute(&mut *tx)
        .await?;
    let operation = format!("{kind}:{entity}");
    let now = time::now_str();
    let id: Option<i64> = sqlx::query_scalar("INSERT INTO integration_deliveries (system_code,case_id,operation_id,kind,payload_json,status,created_at,updated_at) VALUES (?,?,?,?,?,'pending',?,?) ON CONFLICT(operation_id) DO NOTHING RETURNING id")
        .bind(system).bind(case_id).bind(&operation).bind(kind).bind(payload.to_string()).bind(&now).bind(&now).fetch_optional(&mut *tx).await?;
    if let Some(id) = id {
        jobs::enqueue(
            tx,
            "integration.deliver",
            json!({"delivery_id":id}),
            Some(format!("integration.deliver:{id}")),
            chrono::Utc::now(),
        )
        .await?;
        crate::audit::record(
            tx,
            None,
            "records.integration_queued",
            "integration_delivery",
            Some(id),
            json!({"system":system,"operation_id":operation}),
        )
        .await?;
        if let Some(cid) = case_id {
            crate::cases::core::append_event(
                tx,
                cid,
                None,
                "records.integration_queued",
                crate::cases::core::Visibility::Staff,
                "Record queued for delivery to the external system.",
                json!({"delivery_id":id,"system":system}),
            )
            .await?;
        }
    }
    Ok(())
}
#[derive(sqlx::FromRow)]
struct Delivery {
    operation_id: String,
    payload_json: String,
    status: String,
    attempts: i64,
    base_url: String,
    enabled: i64,
    case_id: Option<i64>,
}
#[derive(Deserialize, Serialize)]
pub struct Accepted {
    pub external_ref: String,
}
pub async fn deliver(state: &AppState, id: i64) -> AppResult<()> {
    let mut tx = write_tx(&state.db).await?;
    let delivery: Option<Delivery> = sqlx::query_as("SELECT d.operation_id,d.payload_json,d.status,d.attempts,d.case_id,s.base_url,s.enabled FROM integration_deliveries d JOIN external_systems s ON s.code=d.system_code WHERE d.id=?")
        .bind(id).fetch_optional(&mut *tx).await?;
    let Some(d) = delivery else { return Ok(()) };
    if d.status == "accepted" || d.status == "dead" {
        return Ok(());
    }
    let attempts = d.attempts + 1;
    sqlx::query("UPDATE integration_deliveries SET status='sending', attempts=?,updated_at=? WHERE id=?")
        .bind(attempts)
        .bind(time::fmt(state.now()))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let result = async {
        if d.enabled == 0 {
            return Err(AppError::internal("External system is disabled."));
        }
        let base = if d.base_url.starts_with('/') {
            format!("{}{}", state.cfg.internal_base_url, d.base_url)
        } else {
            d.base_url.clone()
        };
        let payload: Value = serde_json::from_str(&d.payload_json)?;
        let response = state
            .http
            .post(format!("{}/api/records", base.trim_end_matches('/')))
            .timeout(Duration::from_secs(5))
            .header("X-Mock-Key", &state.cfg.mock_api_key)
            .header("Idempotency-Key", &d.operation_id)
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::internal(format!("Records delivery failed: {e}")))?;
        if !response.status().is_success() {
            return Err(AppError::internal(format!("Records receiver returned HTTP {}", response.status())));
        }
        let accepted = response
            .json::<Accepted>()
            .await
            .map_err(|e| AppError::internal(format!("Invalid records receipt: {e}")))?;
        if accepted.external_ref.trim().is_empty() {
            return Err(AppError::internal("Records receipt has no external reference."));
        }
        Ok(accepted)
    }
    .await;
    let mut tx = write_tx(&state.db).await?;
    let now = time::fmt(state.now());
    match &result {
        Ok(accepted) => {
            sqlx::query("UPDATE integration_deliveries SET status='accepted',external_ref=?,last_error=NULL,next_attempt_at=NULL,updated_at=? WHERE id=?")
                .bind(&accepted.external_ref).bind(&now).bind(id).execute(&mut *tx).await?;
            if let Some(cid) = d.case_id {
                common::changed(
                    &mut tx,
                    &Actor::system(),
                    cid,
                    "records.integration_accepted",
                    "Record accepted by the external system.",
                    json!({"delivery_id":id,"external_ref":accepted.external_ref}),
                )
                .await?;
            }
        }
        Err(err) => {
            let status = if attempts >= MAX_ATTEMPTS { "dead" } else { "failed" };
            let next = (status == "failed").then(|| time::fmt(state.now() + jobs::backoff(attempts)));
            sqlx::query(
                "UPDATE integration_deliveries SET status=?,last_error=?,next_attempt_at=?,updated_at=? WHERE id=?",
            )
            .bind(status)
            .bind(&err.message)
            .bind(next)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    result.map(|_| ())
}
#[derive(Default, Deserialize)]
struct Filters {
    status: Option<String>,
    system: Option<String>,
}
async fn list(State(state): State<AppState>, actor: Actor, Query(f): Query<Filters>) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let scope = authz::case_scope_sql(&actor);
    let mut binds = scope.binds;
    let status = f.status.map(SqlValue::Text).unwrap_or(SqlValue::Null);
    let system = f.system.map(SqlValue::Text).unwrap_or(SqlValue::Null);
    binds.extend([status.clone(), status, system.clone(), system]);
    let sql = format!(
        "SELECT d.id,d.system_code,d.operation_id,d.kind,d.status,d.attempts,d.external_ref,d.last_error,d.next_attempt_at,d.updated_at,CASE WHEN c.id IS NOT NULL AND {} THEN d.case_id END AS case_id FROM integration_deliveries d LEFT JOIN cases c ON c.id=d.case_id WHERE (? IS NULL OR d.status=?) AND (? IS NULL OR d.system_code=?) ORDER BY d.id DESC",
        scope.sql
    );
    let mut conn = state.db.acquire().await?;
    Ok(Json(json!(rows(&mut conn, &sql, &binds).await?)))
}
async fn case_deliveries(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    authz::require_staff_case(&mut conn, &actor, id).await?;
    Ok(Json(json!(rows(&mut conn,"SELECT id,system_code,kind,status,attempts,external_ref,last_error,updated_at FROM integration_deliveries WHERE case_id=? ORDER BY id",&[SqlValue::Int(id)]).await?)))
}
async fn systems(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut conn = state.db.acquire().await?;
    Ok(Json(json!(rows(&mut conn,"SELECT s.*,m.outage,m.drop_responses FROM external_systems s JOIN mock_system_state m ON m.system_code=s.code ORDER BY s.code",&[]).await?)))
}
#[derive(Deserialize)]
struct Switches {
    outage: bool,
    drop_responses: bool,
}
async fn toggle(
    State(state): State<AppState>,
    actor: Actor,
    Path(code): Path<String>,
    Json(body): Json<Switches>,
) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut tx = write_tx(&state.db).await?;
    let changed = sqlx::query("UPDATE mock_system_state SET outage=?,drop_responses=? WHERE system_code=?")
        .bind(body.outage)
        .bind(body.drop_responses)
        .bind(&code)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if changed == 0 {
        return Err(AppError::not_found());
    }
    common::admin_audit(
        &mut tx,
        &actor,
        "records.mock_configure",
        "external_system",
        None,
        json!({"code":code,"outage":body.outage,"drop_responses":body.drop_responses}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn retry(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut tx = write_tx(&state.db).await?;
    let running: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM jobs WHERE idempotency_key=? AND status='running')")
            .bind(format!("integration.deliver:{id}"))
            .fetch_one(&mut *tx)
            .await?;
    if running {
        return Err(AppError::conflict("This delivery is still running. Refresh before retrying."));
    }
    let changed=sqlx::query("UPDATE integration_deliveries SET status='pending',attempts=0,last_error=NULL,next_attempt_at=NULL,updated_at=? WHERE id=? AND status IN ('failed','dead')")
        .bind(time::fmt(state.now())).bind(id).execute(&mut *tx).await?.rows_affected();
    if changed == 0 {
        return Err(AppError::conflict("Only failed or stopped deliveries can be retried."));
    }
    // Reset the existing durable job instead of adding a competing sender.
    sqlx::query("UPDATE jobs SET status='pending',attempts=0,lease_until=NULL,last_error=NULL,run_after=?,updated_at=? WHERE idempotency_key=? AND status <> 'running'")
        .bind(time::fmt(state.now())).bind(time::fmt(state.now())).bind(format!("integration.deliver:{id}")).execute(&mut *tx).await?;
    common::admin_audit(&mut tx, &actor, "records.integration_retry", "integration_delivery", Some(id), json!({}))
        .await?;
    let cid: Option<i64> = sqlx::query_scalar("SELECT case_id FROM integration_deliveries WHERE id=?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if let Some(cid) = cid {
        crate::cases::core::append_event(
            &mut tx,
            cid,
            actor.db_id(),
            "records.integration_retry",
            crate::cases::core::Visibility::Staff,
            "External record delivery queued for another attempt.",
            json!({"delivery_id":id}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn remote(State(state): State<AppState>, actor: Actor, Path(code): Path<String>) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut conn = state.db.acquire().await?;
    // The administration console shows receiver metadata; payload is projected through case scope.
    let scope = authz::case_scope_sql(&actor);
    let sql = format!(
        "SELECT r.id,r.operation_id,r.external_ref,r.received_at,CASE WHEN {} THEN r.payload_json END AS payload_json FROM mock_external_records r LEFT JOIN integration_deliveries d ON d.operation_id=r.operation_id LEFT JOIN cases c ON c.id=d.case_id WHERE r.system_code=? ORDER BY r.id",
        scope.sql
    );
    let mut binds = scope.binds;
    binds.push(SqlValue::Text(code));
    Ok(Json(json!(rows(&mut conn, &sql, &binds).await?)))
}
