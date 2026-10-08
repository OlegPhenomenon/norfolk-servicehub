use super::{
    core::{self, Visibility},
    record,
};
use crate::{
    auth::Actor,
    authz::{self, CaseAccess},
    db,
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
use serde::Deserialize;
use serde_json::{Value, json};
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cases/{id}/representatives", get(list).post(add))
        .route("/api/cases/{id}/representatives/{rid}/revoke", post(revoke))
}
#[derive(Deserialize)]
struct Input {
    email: String,
    basis: String,
}
async fn require_applicant(tx: &mut sqlx::SqliteConnection, actor: &Actor, id: i64) -> AppResult<()> {
    let (_, access) = authz::require_case(tx, actor, id).await?;
    if access != CaseAccess::Applicant {
        return Err(AppError::not_found());
    }
    if !may_delegate(tx, actor, id).await? {
        return Err(AppError::forbidden());
    }
    Ok(())
}
/// For an actor with applicant access: the applicant (or an organisation member) may authorise and revoke
/// representatives; a representative can act but cannot delegate their authorisation.
async fn may_delegate(tx: &mut sqlx::SqliteConnection, actor: &Actor, id: i64) -> AppResult<bool> {
    let representative: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM case_representatives WHERE case_id=? AND user_id=? AND status='active')",
    )
    .bind(id)
    .bind(actor.user_id)
    .fetch_one(&mut *tx)
    .await?;
    let case = core::load_case(tx, id).await?;
    if representative && case.applicant_user_id != Some(actor.user_id) {
        let member: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM memberships WHERE organisation_id=? AND user_id=? AND status='active')",
        )
        .bind(case.applicant_org_id)
        .bind(actor.user_id)
        .fetch_one(&mut *tx)
        .await?;
        return Ok(member);
    }
    Ok(true)
}
async fn add(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<Input>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&state.db).await?;
    require_applicant(&mut tx, &actor, id).await?;
    if input.basis.trim().is_empty() {
        return Err(AppError::field("basis", "Describe the authorisation."));
    }
    let uid: i64 =
        sqlx::query_scalar("SELECT id FROM users WHERE email=? COLLATE NOCASE AND kind='resident' AND is_active=1")
            .bind(input.email.trim())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| AppError::field("email", "Choose an existing resident account."))?;
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM case_representatives WHERE case_id=? AND user_id=? AND status='active')",
    )
    .bind(id)
    .bind(uid)
    .fetch_one(&mut *tx)
    .await?;
    if active {
        return Err(AppError::conflict("This representative is already authorised."));
    }
    let rid:i64=sqlx::query_scalar("INSERT INTO case_representatives(case_id,user_id,basis,status,created_at) VALUES (?,?,?,'active',?) RETURNING id").bind(id).bind(uid).bind(&input.basis).bind(time::fmt(state.now())).fetch_one(&mut *tx).await?;
    core::bump_revision(&mut tx, id, None).await?;
    record(
        &mut tx,
        &actor,
        id,
        "representative.added",
        Visibility::Applicant,
        "Applicant authorised a representative.",
        json!({"representative_id":rid,"basis":input.basis}),
    )
    .await?;
    notify::send(
        &mut tx,
        Notice {
            user_id: Some(uid),
            case_id: Some(id),
            subject: "Representative access granted".into(),
            body: "You can now act on this request.".into(),
            link: Some(format!("/my/cases/{id}")),
            ..Notice::default()
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":rid})))
}
async fn revoke(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, rid)): Path<(i64, i64)>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&state.db).await?;
    require_applicant(&mut tx, &actor, id).await?;
    let uid:Option<i64>=sqlx::query_scalar("UPDATE case_representatives SET status='revoked',revoked_at=?,revoked_by=? WHERE id=? AND case_id=? AND status='active' RETURNING user_id").bind(time::fmt(state.now())).bind(actor.db_id()).bind(rid).bind(id).fetch_optional(&mut *tx).await?;
    let uid = uid.ok_or_else(AppError::not_found)?;
    core::bump_revision(&mut tx, id, None).await?;
    record(
        &mut tx,
        &actor,
        id,
        "representative.revoked",
        Visibility::Applicant,
        "Representative access revoked immediately.",
        json!({"representative_id":rid}),
    )
    .await?;
    notify::send(
        &mut tx,
        Notice {
            user_id: Some(uid),
            subject: "Representative access revoked".into(),
            body: "Your authority to act on a request was revoked by the applicant.".into(),
            ..Notice::default()
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"revoked":true})))
}
async fn list(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut tx = state.db.acquire().await?;
    let (_, access) = authz::require_case(&mut tx, &actor, id).await?;
    if matches!(access, CaseAccess::TaskOnly | CaseAccess::None) {
        return Err(AppError::not_found());
    }
    let can_manage = access == CaseAccess::Applicant && may_delegate(&mut tx, &actor, id).await?;
    let rows:Vec<(i64,String,String,String)>=sqlx::query_as("SELECT r.id,u.display_name,r.basis,r.status FROM case_representatives r JOIN users u ON u.id=r.user_id WHERE case_id=? ORDER BY r.id").bind(id).fetch_all(&mut *tx).await?;
    Ok(Json(
        json!({"items":rows.into_iter().map(|(id,name,basis,status)|json!({"id":id,"name":name,"basis":basis,"status":status})).collect::<Vec<_>>(),"can_manage_representatives":can_manage}),
    ))
}
