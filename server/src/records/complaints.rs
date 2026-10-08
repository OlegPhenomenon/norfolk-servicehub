//! Complaint subjects deny every role, including managers. Reviews preserve both histories.
use super::common::{self, rows};
use crate::{
    auth::Actor,
    authz::{self, CaseAccess, Role},
    cases::core::{self, CaseRow, Visibility},
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
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cases/{id}/complaint", get(panel))
        .route("/api/cases/{id}/complaint/subjects", post(subjects))
        .route("/api/cases/{id}/complaint/request-review", post(request_review))
}
pub async fn select_handler(tx: &mut SqliteConnection, id: i64, exclude: Option<i64>) -> AppResult<Option<i64>> {
    Ok(sqlx::query_scalar("SELECT u.id FROM users u WHERE u.kind='staff' AND u.is_active=1 AND (? IS NULL OR u.id<>?) AND NOT EXISTS(SELECT 1 FROM case_access_denials d WHERE d.case_id=? AND d.user_id=u.id) AND EXISTS(SELECT 1 FROM role_grants g WHERE g.user_id=u.id AND g.revoked_at IS NULL AND g.role IN ('complaints_officer','manager')) ORDER BY EXISTS(SELECT 1 FROM role_grants g WHERE g.user_id=u.id AND g.revoked_at IS NULL AND g.role='complaints_officer') DESC,u.persona_key='ruth' DESC,u.id LIMIT 1")
        .bind(exclude).bind(exclude).bind(id).fetch_optional(tx).await?)
}
/// Tells the staff handler a confidential case is theirs; `what` is "complaint" or "independent review".
/// The text carries only the reference, so it bypasses the generic confidential rewrite.
pub async fn notify_handler(tx: &mut SqliteConnection, case: &CaseRow, uid: i64, what: &str) -> AppResult<()> {
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id=?").bind(uid).fetch_one(&mut *tx).await?;
    let message = format!("Confidential {what} {} assigned to you", case.number.as_deref().unwrap_or("request"));
    notify::send_reference_only(
        tx,
        Notice {
            user_id: Some(uid),
            email: Some(email),
            case_id: Some(case.id),
            subject: message.clone(),
            body: format!("{message} — sign in to read it."),
            link: Some(format!("/staff/cases/{}", case.id)),
            ..Notice::default()
        },
    )
    .await
}
async fn panel(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut tx = state.db.begin().await?;
    let (case, access) = authz::require_case(&mut tx, &actor, id).await?;
    if case.module != "complaint" || matches!(access, CaseAccess::None | CaseAccess::TaskOnly) {
        return Err(AppError::not_found());
    }
    let staff = access.is_staff();
    let scope = authz::case_scope_sql(&actor);
    let mut binds = vec![SqlValue::Int(id), SqlValue::Int(id), SqlValue::Int(id)];
    binds.extend(scope.binds);
    let links=rows(&mut tx,&format!("SELECT c.id,c.number,c.title,l.kind FROM case_links l JOIN cases c ON c.id=CASE WHEN l.from_case_id=? THEN l.to_case_id ELSE l.from_case_id END WHERE (l.from_case_id=? OR l.to_case_id=?) AND {} ORDER BY l.id",scope.sql),&binds).await?;
    let mut data = json!({"case":{"id":id,"status":case.status,"revision":case.revision},"links":links,"can_triage":staff&&(actor.has_role(Role::ComplaintsOfficer)||actor.has_role(Role::Manager)),"can_request_review":access==CaseAccess::Applicant&&case.status=="completed"});
    if staff {
        data["hidden_from"]=json!(rows(&mut tx,"SELECT u.id,u.display_name,d.reason FROM case_access_denials d JOIN users u ON u.id=d.user_id WHERE d.case_id=? ORDER BY u.display_name",&[SqlValue::Int(id)]).await?);
        data["staff"] = json!(
            rows(
                &mut tx,
                "SELECT id,display_name FROM users WHERE kind='staff' AND is_active=1 ORDER BY display_name",
                &[]
            )
            .await?
        );
        data["subjects"] = json!(
            sqlx::query_scalar::<_, i64>(
                "SELECT staff_user_id FROM complaint_subjects WHERE case_id=? ORDER BY staff_user_id"
            )
            .bind(id)
            .fetch_all(&mut *tx)
            .await?
        );
    }
    tx.commit().await?;
    Ok(Json(data))
}
#[derive(Deserialize)]
pub struct Subjects {
    pub staff_user_ids: Vec<i64>,
    pub expected_revision: Option<i64>,
}
pub async fn subjects(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(body): Json<Subjects>,
) -> AppResult<Json<Value>> {
    let mut tx = write_tx(&state.db).await?;
    let (case, _) = authz::require_staff_case(&mut tx, &actor, id).await?;
    if case.module != "complaint" {
        return Err(AppError::not_found());
    }
    actor.require_any_role(&[Role::ComplaintsOfficer, Role::Manager])?;
    if body.staff_user_ids.len() > 100 {
        return Err(AppError::field("staff_user_ids", "Choose at most 100 staff members."));
    }
    let mut ids = body.staff_user_ids;
    ids.sort_unstable();
    ids.dedup();
    if ids.contains(&actor.user_id) {
        return Err(AppError::conflict("Ask another complaints officer to record a complaint about you."));
    }
    for uid in &ids {
        let staff: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=? AND kind='staff')")
            .bind(uid)
            .fetch_one(&mut *tx)
            .await?;
        if !staff {
            return Err(AppError::field("staff_user_ids", "Choose existing staff members."));
        }
    }
    common::check_revision(&case, body.expected_revision)?;
    // This command adds exclusions; removing an exclusion needs a separately audited decision.
    for uid in &ids {
        sqlx::query("INSERT INTO complaint_subjects(case_id,staff_user_id) VALUES(?,?) ON CONFLICT DO NOTHING")
            .bind(id)
            .bind(uid)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO case_access_denials(case_id,user_id,reason,created_by,created_at) VALUES(?,?,'Subject of complaint',?,?) ON CONFLICT DO NOTHING").bind(id).bind(uid).bind(actor.user_id).bind(time::fmt(state.now())).execute(&mut *tx).await?;
    }
    let owner: Option<i64> = sqlx::query_scalar(
        "SELECT user_id FROM case_assignments WHERE case_id=? AND role='owner' AND ended_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    if owner.is_some_and(|uid| ids.contains(&uid)) {
        let replacement = select_handler(&mut tx, id, owner)
            .await?
            .ok_or_else(|| AppError::conflict("No independent handler is available."))?;
        crate::cases::api::assign_owner(
            &state,
            &mut tx,
            &actor,
            id,
            replacement,
            "Previous handler is a subject of the complaint.",
        )
        .await?;
        notify_handler(&mut tx, &case, replacement, "complaint").await?;
    }
    common::changed(
        &mut tx,
        &actor,
        id,
        "complaint.subjects",
        "Complaint subjects recorded and their access removed.",
        json!({"staff_user_ids":ids}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct Review {
    reason: String,
}
async fn request_review(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(body): Json<Review>,
) -> AppResult<Json<Value>> {
    let reason = common::text(&body.reason, "reason", 5000)?;
    let mut tx = write_tx(&state.db).await?;
    let (case, access) = authz::require_case(&mut tx, &actor, id).await?;
    if case.module != "complaint" || access != CaseAccess::Applicant {
        return Err(AppError::not_found());
    }
    if case.status != "completed" {
        return Err(AppError::conflict("A review can be requested after the complaint is completed."));
    }
    let existing:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM case_links l JOIN cases c ON c.id=l.from_case_id WHERE l.to_case_id=? AND l.kind='review_of' AND c.closed_at IS NULL)").bind(id).fetch_one(&mut *tx).await?;
    if existing {
        return Err(AppError::conflict("A review of this feedback is already open."));
    }
    let review = crate::cases::api::create_review(&mut tx, &actor, &case, &reason).await?;
    sqlx::query("UPDATE cases SET confidential=1 WHERE id=?").bind(review.id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO complaint_subjects(case_id,staff_user_id) SELECT ?,staff_user_id FROM complaint_subjects WHERE case_id=?").bind(review.id).bind(id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO case_access_denials(case_id,user_id,reason,created_by,created_at) SELECT ?,user_id,reason,?,? FROM case_access_denials WHERE case_id=?").bind(review.id).bind(actor.user_id).bind(time::fmt(state.now())).bind(id).execute(&mut *tx).await?;
    let original: Option<i64> = sqlx::query_scalar(
        "SELECT user_id FROM case_assignments WHERE case_id=? AND role='owner' ORDER BY id DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let handler = select_handler(&mut tx, review.id, original)
        .await?
        .ok_or_else(|| AppError::conflict("No independent reviewer is available."))?;
    crate::cases::api::assign_owner(
        &state,
        &mut tx,
        &actor,
        review.id,
        handler,
        "Independent review of completed feedback.",
    )
    .await?;
    common::changed(
        &mut tx,
        &actor,
        id,
        "complaint.review_requested",
        "The applicant requested an independent review.",
        json!({"review_case_id":review.id}),
    )
    .await?;
    core::append_event(
        &mut tx,
        review.id,
        actor.db_id(),
        "complaint.review",
        Visibility::Applicant,
        "Independent review requested.",
        json!({"review_of":id,"reason":reason}),
    )
    .await?;
    notify_handler(&mut tx, &review, handler, "independent review").await?;
    tx.commit().await?;
    Ok(Json(json!({"id":review.id,"number":review.number})))
}
