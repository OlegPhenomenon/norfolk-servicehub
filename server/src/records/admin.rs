//! Configuration commands use the platform's user/settings/notification helpers.
use super::common::{self, require_role, rows};
use crate::{
    auth::{
        Actor, UserKind,
        users::{self, NewUser},
    },
    authz::Role,
    db::{SqlValue, write_tx},
    error::{AppError, AppResult},
    notify::{self, Notice},
    settings,
    state::AppState,
    web::{Json, Path},
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
        .route("/api/admin/users", get(list_users).post(create_user))
        .route("/api/admin/users/{id}/deactivate", post(deactivate))
        .route("/api/admin/users/{id}/roles", get(roles).post(grant_role))
        .route("/api/admin/users/{id}/roles/{grant}/revoke", post(revoke_role))
        .route("/api/staff/decision-authorities", get(authorities).post(grant_authority))
        .route("/api/admin/settings", get(get_settings).post(save_settings))
        .route("/api/admin/deliveries", get(deliveries))
        .route("/api/admin/deliveries/{id}/retry", post(retry_notification))
        .route("/api/admin/backups", get(backups))
}
async fn list_users(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut conn = state.db.acquire().await?;
    Ok(Json(json!(
        rows(
            &mut conn,
            "SELECT id,email,display_name,kind,job_title,is_active,totp_enabled FROM users ORDER BY display_name",
            &[]
        )
        .await?
    )))
}
#[derive(Deserialize)]
struct CreateUser {
    email: String,
    display_name: String,
    kind: UserKind,
    password: String,
    job_title: Option<String>,
}
async fn create_user(
    State(state): State<AppState>,
    actor: Actor,
    Json(body): Json<CreateUser>,
) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let email = common::email(&body.email)?;
    let name = common::text(&body.display_name, "display_name", 160)?;
    if body.password.len() < 12 || body.password.len() > 200 {
        return Err(AppError::field("password", "Use a password between 12 and 200 characters."));
    }
    let password = crate::auth::password::hash(&body.password)?;
    let mut tx = write_tx(&state.db).await?;
    let id = users::create_user(
        &mut tx,
        NewUser {
            email,
            display_name: name,
            phone: None,
            kind: body.kind,
            password_hash: Some(password),
            totp_secret: None,
            totp_enabled: false,
            persona_key: None,
            job_title: body.job_title,
        },
    )
    .await?;
    common::admin_audit(&mut tx, &actor, "user.create", "user", Some(id), json!({"kind":body.kind})).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
async fn deactivate(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    if id == actor.user_id {
        return Err(AppError::conflict("You cannot deactivate the account you are using."));
    }
    let mut tx = write_tx(&state.db).await?;
    crate::auth::users::deactivate_user(&mut tx, id).await?;
    common::admin_audit(&mut tx, &actor, "user.deactivate", "user", Some(id), json!({})).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn roles(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut conn = state.db.acquire().await?;
    Ok(Json(json!(
        rows(
            &mut conn,
            "SELECT id,role,scope_service_id,granted_at,revoked_at FROM role_grants WHERE user_id=? ORDER BY id",
            &[SqlValue::Int(id)]
        )
        .await?
    )))
}
#[derive(Deserialize)]
struct GrantRole {
    role: Role,
    scope_service_id: Option<i64>,
}
async fn grant_role(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(body): Json<GrantRole>,
) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut tx = write_tx(&state.db).await?;
    let grant = users::grant_role(&mut tx, id, body.role, body.scope_service_id, actor.db_id()).await?;
    common::admin_audit(
        &mut tx,
        &actor,
        "user.grant_role",
        "role_grant",
        Some(grant),
        json!({"user_id":id,"role":body.role}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":grant})))
}
async fn revoke_role(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, grant)): Path<(i64, i64)>,
) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut tx = write_tx(&state.db).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM role_grants WHERE id=? AND user_id=?)")
        .bind(grant)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if !exists {
        return Err(AppError::not_found());
    }
    users::revoke_role(&mut tx, grant).await?;
    common::admin_audit(&mut tx, &actor, "user.revoke_role", "role_grant", Some(grant), json!({"user_id":id})).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn authorities(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Manager)?;
    let mut conn = state.db.acquire().await?;
    Ok(Json(
        json!({"authorities":rows(&mut conn,"SELECT a.*,u.display_name FROM decision_authorities a JOIN users u ON u.id=a.user_id WHERE a.revoked_at IS NULL ORDER BY a.id",&[]).await?,"staff":rows(&mut conn,"SELECT id,display_name FROM users WHERE kind='staff' AND is_active=1 ORDER BY display_name",&[]).await?}),
    ))
}
#[derive(Deserialize)]
struct GrantAuthority {
    user_id: i64,
    decision_type: String,
    service_id: Option<i64>,
}
async fn grant_authority(
    State(state): State<AppState>,
    actor: Actor,
    Json(body): Json<GrantAuthority>,
) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Manager)?;
    if ![
        "development_approval",
        "building_approval",
        "modification_approval",
        "planning_certificate",
        "complaint_response",
        "road_response",
    ]
    .contains(&body.decision_type.as_str())
    {
        return Err(AppError::field("decision_type", "Choose a recognised decision type."));
    }
    let mut tx = write_tx(&state.db).await?;
    let staff: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=? AND kind='staff' AND is_active=1)")
            .bind(body.user_id)
            .fetch_one(&mut *tx)
            .await?;
    if !staff {
        return Err(AppError::field("user_id", "Choose an active staff member."));
    }
    let id =
        users::grant_decision_authority(&mut tx, body.user_id, &body.decision_type, body.service_id, actor.user_id)
            .await?;
    common::admin_audit(
        &mut tx,
        &actor,
        "user.grant_authority",
        "decision_authority",
        Some(id),
        json!({"user_id":body.user_id,"decision_type":body.decision_type}),
    )
    .await?;
    let email: String =
        sqlx::query_scalar("SELECT email FROM users WHERE id=?").bind(body.user_id).fetch_one(&mut *tx).await?;
    notify::send(
        &mut tx,
        Notice {
            user_id: Some(body.user_id),
            email: Some(email),
            subject: "Decision authority granted".into(),
            body: format!("A manager has granted you authority for {}.", body.decision_type.replace('_', " ")),
            link: Some("/staff".into()),
            ..Notice::default()
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
const ADDRESSES: [&str; 3] = ["notify.customer_care_email", "notify.finance_email", "notify.works_depot_email"];
async fn get_settings(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut conn = state.db.acquire().await?;
    let mut object = serde_json::Map::new();
    for key in ADDRESSES {
        object.insert(key.into(), settings::get_json(&mut conn, key).await?.unwrap_or(json!("")));
    }
    object.insert("demo.reset_hours".into(), json!(state.cfg.demo_reset_hours));
    Ok(Json(Value::Object(object)))
}
async fn save_settings(
    State(state): State<AppState>,
    actor: Actor,
    Json(body): Json<std::collections::BTreeMap<String, String>>,
) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    for (key, value) in &body {
        if !ADDRESSES.contains(&key.as_str()) {
            return Err(AppError::field(key, "This setting is read-only or unknown."));
        }
        common::email(value).map_err(|_| AppError::field(key, "Enter a valid email address."))?;
    }
    let mut tx = write_tx(&state.db).await?;
    for (key, value) in &body {
        settings::set(&mut tx, key, value, actor.db_id()).await?;
    }
    common::admin_audit(
        &mut tx,
        &actor,
        "settings.update",
        "settings",
        None,
        json!({"keys":body.keys().collect::<Vec<_>>()}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn deliveries(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut conn = state.db.acquire().await?;
    Ok(Json(json!(rows(&mut conn,"SELECT n.id,n.channel,n.to_address,CASE WHEN j.status='dead' AND n.status='queued' THEN 'failed' ELSE n.status END AS status,n.attempts,n.last_error,n.external_id,n.created_at,n.sent_at FROM notifications n LEFT JOIN jobs j ON j.idempotency_key='notify.deliver:'||n.id WHERE n.channel<>'in_app' ORDER BY n.id DESC",&[]).await?)))
}
async fn retry_notification(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut tx = write_tx(&state.db).await?;
    let n = rows(
        &mut tx,
        "SELECT n.* FROM notifications n WHERE n.id=? AND n.channel<>'in_app' AND (n.status='failed' OR (n.status='queued' AND EXISTS(SELECT 1 FROM jobs j WHERE j.idempotency_key='notify.deliver:'||n.id AND j.status='dead')))",
        &[SqlValue::Int(id)],
    )
    .await?;
    let n = n.first().ok_or_else(|| AppError::conflict("Only failed notifications can be retried."))?;
    notify::send(
        &mut tx,
        Notice {
            user_id: None,
            email: (n["channel"] == "email").then(|| n["to_address"].as_str().unwrap_or_default().into()),
            phone: (n["channel"] == "sms").then(|| n["to_address"].as_str().unwrap_or_default().into()),
            case_id: n["case_id"].as_i64(),
            subject: n["subject"].as_str().unwrap_or_default().into(),
            body: n["body"].as_str().unwrap_or_default().into(),
            link: n["link"].as_str().map(String::from),
        },
    )
    .await?;
    common::admin_audit(
        &mut tx,
        &actor,
        "notification.retry",
        "notification",
        Some(id),
        json!({"retry":"new_delivery_preserves_original"}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn backups(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut conn = state.db.acquire().await?;
    Ok(Json(
        json!({"runs":rows(&mut conn,"SELECT * FROM backup_runs ORDER BY id DESC LIMIT 50",&[]).await?,"last_backup":rows(&mut conn,"SELECT * FROM backup_runs WHERE kind='backup' AND status='ok' ORDER BY id DESC LIMIT 1",&[]).await?.first(),"last_verified_restore":rows(&mut conn,"SELECT * FROM backup_runs WHERE kind='restore_check' AND status='ok' ORDER BY id DESC LIMIT 1",&[]).await?.first()}),
    ))
}
