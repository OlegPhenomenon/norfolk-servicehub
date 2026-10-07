//! Invitations and revocations are checked against live membership on every request.
use super::common::{self, rows};
use crate::{
    auth::Actor,
    db::{SqlValue, write_tx},
    error::{AppError, AppResult},
    notify::{self, Notice},
    state::AppState,
    time,
    web::{Json, Path},
};
use axum::{
    Router,
    extract::State,
    routing::{get, post},
};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/my/organisations", get(list).post(create))
        .route("/api/my/organisations/{id}/invites", post(invite))
        .route("/api/my/invites/{token}/accept", post(accept))
        .route("/api/my/organisations/{id}/members/{mid}/revoke", post(revoke))
}
async fn require_owner(tx: &mut SqliteConnection, actor: &Actor, id: i64) -> AppResult<()> {
    let ok:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM memberships WHERE organisation_id=? AND user_id=? AND role='owner' AND status='active')").bind(id).bind(actor.user_id).fetch_one(tx).await?;
    if !ok {
        return Err(AppError::not_found());
    }
    Ok(())
}
async fn list(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    let mut orgs=rows(&mut conn,"SELECT o.*,m.role FROM organisations o JOIN memberships m ON m.organisation_id=o.id WHERE m.user_id=? AND m.status='active' ORDER BY o.name",&[SqlValue::Int(actor.user_id)]).await?;
    for org in &mut orgs {
        org["members"]=json!(rows(&mut conn,"SELECT m.id,m.user_id,m.invite_email,m.role,m.status,u.display_name FROM memberships m LEFT JOIN users u ON u.id=m.user_id WHERE m.organisation_id=? ORDER BY m.id",&[SqlValue::Int(org["id"].as_i64().unwrap())]).await?);
    }
    Ok(Json(json!(orgs)))
}
#[derive(Deserialize)]
struct Invite {
    email: String,
}
async fn invite(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(body): Json<Invite>,
) -> AppResult<Json<Value>> {
    let email = common::email(&body.email)?;
    let mut token = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut token);
    let token = hex::encode(token);
    let hash = hex::encode(Sha256::digest(token.as_bytes()));
    let mut tx = write_tx(&state.db).await?;
    require_owner(&mut tx, &actor, id).await?;
    let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM memberships WHERE organisation_id=? AND invite_email=? AND status IN ('active','invited'))").bind(id).bind(&email).fetch_one(&mut *tx).await?;
    if exists {
        return Err(AppError::conflict("This person is already a member or has a pending invitation."));
    }
    let mid:i64=sqlx::query_scalar("INSERT INTO memberships(organisation_id,invite_email,invite_token_hash,role,status,invited_by,created_at) VALUES(?,?,?,'member','invited',?,?) RETURNING id").bind(id).bind(&email).bind(hash).bind(actor.user_id).bind(time::fmt(state.now())).fetch_one(&mut *tx).await?;
    let name: String =
        sqlx::query_scalar("SELECT name FROM organisations WHERE id=?").bind(id).fetch_one(&mut *tx).await?;
    let link = format!("/my/invites/{token}");
    notify::send(
        &mut tx,
        Notice {
            email: Some(email.clone()),
            subject: format!("Invitation to {name}"),
            body: format!(
                "You have been invited to {name}. Sign in with {email} and accept the invitation at {}{link}.",
                state.cfg.public_base_url
            ),
            link: Some(link),
            ..Notice::default()
        },
    )
    .await?;
    common::admin_audit(
        &mut tx,
        &actor,
        "organisation.invite",
        "membership",
        Some(mid),
        json!({"organisation_id":id,"email":email}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":mid,"status":"invited"})))
}
async fn accept(State(state): State<AppState>, actor: Actor, Path(token): Path<String>) -> AppResult<Json<Value>> {
    let mut tx = write_tx(&state.db).await?;
    let hash = hex::encode(Sha256::digest(token.as_bytes()));
    let invitation: Option<(i64, i64, String, String)> =
        sqlx::query_as("SELECT id,organisation_id,invite_email,status FROM memberships WHERE invite_token_hash=?")
            .bind(hash)
            .fetch_optional(&mut *tx)
            .await?;
    let (id, org, email, status) = invitation.ok_or_else(AppError::not_found)?;
    let account: String =
        sqlx::query_scalar("SELECT email FROM users WHERE id=?").bind(actor.user_id).fetch_one(&mut *tx).await?;
    if !account.eq_ignore_ascii_case(&email) {
        return Err(AppError::not_found());
    }
    if status != "invited" {
        return Err(AppError::conflict("This invitation is no longer available."));
    }
    sqlx::query("UPDATE memberships SET status='active',user_id=?,accepted_at=?,invite_token_hash=NULL WHERE id=?")
        .bind(actor.user_id)
        .bind(time::fmt(state.now()))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    common::admin_audit(&mut tx, &actor, "organisation.accept", "membership", Some(id), json!({"organisation_id":org}))
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"organisation_id":org})))
}
async fn revoke(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, mid)): Path<(i64, i64)>,
) -> AppResult<Json<Value>> {
    let mut tx = write_tx(&state.db).await?;
    require_owner(&mut tx, &actor, id).await?;
    let member:Option<(Option<i64>,String,String)>=sqlx::query_as("SELECT user_id,invite_email,role FROM memberships WHERE id=? AND organisation_id=? AND status IN ('active','invited')").bind(mid).bind(id).fetch_optional(&mut *tx).await?;
    let (uid, email, role) = member.ok_or_else(AppError::not_found)?;
    if role == "owner" {
        return Err(AppError::conflict("An organisation owner cannot be revoked here."));
    }
    sqlx::query("UPDATE memberships SET status='revoked',invite_token_hash=NULL,revoked_at=?,revoked_by=? WHERE id=?")
        .bind(time::fmt(state.now()))
        .bind(actor.user_id)
        .bind(mid)
        .execute(&mut *tx)
        .await?;
    common::admin_audit(&mut tx, &actor, "organisation.revoke", "membership", Some(mid), json!({"organisation_id":id}))
        .await?;
    notify::send(
        &mut tx,
        Notice {
            user_id: uid,
            email: Some(email),
            subject: "Organisation access removed".into(),
            body: "Your access to this organisation's requests has been removed.".into(),
            ..Notice::default()
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
struct Create {
    name: String,
    abn: Option<String>,
}
async fn create(State(state): State<AppState>, actor: Actor, Json(body): Json<Create>) -> AppResult<Json<Value>> {
    if actor.is_staff() {
        return Err(AppError::forbidden_msg("Use a resident account to create an organisation."));
    }
    let name = common::text(&body.name, "name", 160)?;
    let abn = body.abn.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    if abn.as_ref().is_some_and(|v| v.len() > 40) {
        return Err(AppError::field("abn", "Use at most 40 characters."));
    }
    let now = time::fmt(state.now());
    let mut tx = write_tx(&state.db).await?;
    let email: String =
        sqlx::query_scalar("SELECT email FROM users WHERE id=?").bind(actor.user_id).fetch_one(&mut *tx).await?;
    let id: i64 = sqlx::query_scalar("INSERT INTO organisations(name,abn,created_at) VALUES(?,?,?) RETURNING id")
        .bind(name)
        .bind(abn)
        .bind(&now)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO memberships(organisation_id,user_id,invite_email,role,status,created_at,accepted_at) VALUES(?,?,?,'owner','active',?,?)").bind(id).bind(actor.user_id).bind(email).bind(&now).bind(&now).execute(&mut *tx).await?;
    common::admin_audit(&mut tx, &actor, "organisation.created", "organisation", Some(id), json!({})).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
