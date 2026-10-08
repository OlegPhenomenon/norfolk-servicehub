use super::{
    core::{self, CaseRow, NewCase, Visibility},
    record, require_edit,
};
use crate::{
    auth::{Actor, UserKind},
    authz::{self, CaseAccess, Role},
    db,
    error::{AppError, AppResult},
    services::definition,
    state::AppState,
    time,
    web::{Json, Path},
};
use axum::{
    Router,
    extract::State,
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/services/{slug}/drafts", post(create))
        .route("/api/cases/{id}/draft", put(save).get(load).delete(remove))
        .route("/api/my/drafts", get(list))
}
#[derive(Debug, Default, Deserialize)]
pub struct Applicant {
    pub applicant_name: Option<String>,
    pub applicant_email: Option<String>,
    pub applicant_phone: Option<String>,
    pub applicant_org_id: Option<i64>,
    pub channel: Option<String>,
}
pub async fn create_draft(
    tx: &mut SqliteConnection,
    actor: &Actor,
    slug: &str,
    input: Applicant,
) -> AppResult<CaseRow> {
    let (service_id,version_id,module,name):(i64,i64,String,String)=sqlx::query_as("SELECT s.id,v.id,s.module,s.name FROM services s JOIN service_versions v ON v.service_id=s.id AND v.status='published' WHERE s.slug=? AND s.is_active=1").bind(slug).fetch_optional(&mut *tx).await?.ok_or_else(AppError::not_found)?;
    let assisted = actor.kind == UserKind::Staff;
    if assisted {
        actor.require_any_role(&[Role::Intake])?;
        if !actor.roles_for_service(service_id).contains(&Role::Intake) {
            return Err(AppError::forbidden());
        }
    }
    let (email, phone): (String, Option<String>) =
        sqlx::query_as("SELECT email,phone FROM users WHERE id=? AND is_active=1")
            .bind(actor.user_id)
            .fetch_one(&mut *tx)
            .await?;
    if let Some(org) = input.applicant_org_id {
        let member: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM memberships WHERE organisation_id=? AND user_id=? AND status='active')",
        )
        .bind(org)
        .bind(actor.user_id)
        .fetch_one(&mut *tx)
        .await?;
        if !member {
            return Err(AppError::forbidden());
        }
    }
    let channel = input.channel.unwrap_or_else(|| if assisted { "phone".into() } else { "online".into() });
    if !["online", "phone", "walk_in", "email", "post"].contains(&channel.as_str())
        || (!assisted && channel != "online")
    {
        return Err(AppError::field("channel", "Choose a valid intake channel."));
    }
    let applicant_name = if assisted {
        input
            .applicant_name
            .filter(|n| !n.trim().is_empty())
            .ok_or_else(|| AppError::field("applicant_name", "Enter the applicant's name."))?
    } else {
        actor.display_name.clone()
    };
    if assisted
        && input.applicant_email.as_deref().is_none_or(|s| s.trim().is_empty())
        && input.applicant_phone.as_deref().is_none_or(|s| s.trim().is_empty())
    {
        return Err(AppError::field("applicant_phone", "Enter an email address or phone number."));
    }
    let case = core::create_case(
        tx,
        NewCase {
            service_id,
            service_version_id: version_id,
            module,
            title: name,
            status: "draft".into(),
            applicant_user_id: (!assisted).then_some(actor.user_id),
            applicant_org_id: input.applicant_org_id,
            applicant_name,
            applicant_email: if assisted { input.applicant_email } else { Some(email) },
            applicant_phone: if assisted { input.applicant_phone } else { phone },
            intake_channel: channel,
            recorded_by_user_id: assisted.then_some(actor.user_id),
            property_ref: None,
        },
    )
    .await?;
    sqlx::query("INSERT INTO case_drafts(case_id,updated_at) VALUES (?,?)")
        .bind(case.id)
        .bind(time::now_str())
        .execute(&mut *tx)
        .await?;
    record(tx, actor, case.id, "draft.created", Visibility::Applicant, "Request draft created.", json!({})).await?;
    core::reindex_search(tx, case.id).await?;
    Ok(case)
}
async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Path(slug): Path<String>,
    Json(input): Json<Applicant>,
) -> AppResult<Json<CaseRow>> {
    let mut tx = db::write_tx(&state.db).await?;
    let case = create_draft(&mut tx, &actor, &slug, input).await?;
    tx.commit().await?;
    Ok(Json(case))
}
/// Where a draft's pinned service version stands. Publishing a version (staff, or a catalogue upgrade) retires
/// the previous one; a draft started on it moves to the current version before it is saved or submitted, so
/// no request is ever submitted on a retired version. Submitted cases keep their version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pinned {
    /// The draft is on the published version.
    Current,
    /// The draft's version (number `from`) was retired; `id`/`version` is the published version of the service.
    Replaced { from: i64, id: i64, version: i64 },
    /// The service has no published version: the draft is kept but cannot be submitted.
    Unavailable,
}
pub async fn pinned(conn: &mut SqliteConnection, case: &CaseRow) -> AppResult<Pinned> {
    let (status, from): (String, i64) = sqlx::query_as("SELECT status,version FROM service_versions WHERE id=?")
        .bind(case.service_version_id)
        .fetch_one(&mut *conn)
        .await?;
    if status == "published" {
        return Ok(Pinned::Current);
    }
    let current: Option<(i64, i64)> =
        sqlx::query_as("SELECT id,version FROM service_versions WHERE service_id=? AND status='published'")
            .bind(case.service_id)
            .fetch_optional(&mut *conn)
            .await?;
    Ok(current.map_or(Pinned::Unavailable, |(id, version)| Pinned::Replaced { from, id, version }))
}
/// Moves a draft off a retired version onto the current published version of its service (inside the caller's
/// write transaction) and tells the applicant to review the answers. Only drafts are ever rebound.
pub async fn rebind(tx: &mut SqliteConnection, actor: &Actor, case: &CaseRow) -> AppResult<Pinned> {
    if case.status != "draft" {
        return Ok(Pinned::Current);
    }
    let pinned = pinned(tx, case).await?;
    if let Pinned::Replaced { from, id, version } = pinned {
        sqlx::query("UPDATE cases SET service_version_id=?,updated_at=? WHERE id=? AND status='draft'")
            .bind(id)
            .bind(time::now_str())
            .bind(case.id)
            .execute(&mut *tx)
            .await?;
        record(
            tx,
            actor,
            case.id,
            "draft.version_updated",
            Visibility::Applicant,
            "The form was updated to the current version; please review your answers.",
            json!({"from_version":from,"to_version":version}),
        )
        .await?;
    }
    Ok(pinned)
}
/// Client flags for a draft whose form changed since it was started.
fn form_update(pinned: Pinned) -> Value {
    match pinned {
        Pinned::Replaced { from, version, .. } => {
            json!({"form_updated":true,"version_changed_from":from,"version":version})
        }
        Pinned::Current | Pinned::Unavailable => json!({"form_updated":false}),
    }
}
#[derive(Deserialize)]
pub struct DraftAnswers {
    pub answers: Value,
}
/// Saves draft answers as given (validation happens on submission). Returns the rebind outcome.
pub async fn save_answers(tx: &mut SqliteConnection, actor: &Actor, id: i64, answers: &Value) -> AppResult<Pinned> {
    let case = require_edit(tx, actor, id).await?;
    if case.status != "draft" {
        return Err(AppError::conflict("Only drafts can be edited."));
    }
    if !answers.is_object() {
        return Err(AppError::field("answers", "Answers must be an object."));
    }
    let pinned = rebind(tx, actor, &case).await?;
    sqlx::query("UPDATE case_drafts SET answers_json=?,updated_at=? WHERE case_id=?")
        .bind(answers.to_string())
        .bind(time::now_str())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    core::bump_revision(tx, id, None).await?;
    core::reindex_search(tx, id).await?;
    record(tx, actor, id, "draft.saved", Visibility::Applicant, "Request draft saved.", json!({})).await?;
    Ok(pinned)
}
async fn save(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<DraftAnswers>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&state.db).await?;
    let pinned = save_answers(&mut tx, &actor, id, &input.answers).await?;
    tx.commit().await?;
    let mut body = form_update(pinned);
    body["saved"] = json!(true);
    Ok(Json(body))
}
/// Read-only: a draft on a retired version is shown with the current published definition and flagged
/// `form_updated`; the next save or the submission moves it onto that version.
async fn load(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut tx = state.db.acquire().await?;
    let case = require_edit(&mut tx, &actor, id).await?;
    if case.status != "draft" {
        return Err(AppError::conflict("This request has already been submitted."));
    }
    let answers: String =
        sqlx::query_scalar("SELECT answers_json FROM case_drafts WHERE case_id=?").bind(id).fetch_one(&mut *tx).await?;
    let pinned = pinned(&mut tx, &case).await?;
    let def = match pinned {
        Pinned::Replaced { id, .. } => {
            definition::load_for_case(&mut tx, &CaseRow { service_version_id: id, ..case.clone() }).await?
        }
        Pinned::Current | Pinned::Unavailable => definition::load_for_case(&mut tx, &case).await?,
    };
    let mut body = form_update(pinned);
    body["case"] = json!(case);
    body["definition"] = json!(def);
    body["answers"] = serde_json::from_str::<Value>(&answers)?;
    Ok(Json(body))
}
async fn remove(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&state.db).await?;
    let case = require_edit(&mut tx, &actor, id).await?;
    if case.status != "draft" {
        return Err(AppError::conflict("Only a draft can be deleted."));
    }
    // Keep its document/audit references intact; remove saved answers and retire the draft.
    sqlx::query("DELETE FROM case_drafts WHERE case_id=?").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE cases SET status='withdrawn',closed_at=?,updated_at=? WHERE id=?")
        .bind(time::now_str())
        .bind(time::now_str())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    record(&mut tx, &actor, id, "draft.deleted", Visibility::Applicant, "Request draft deleted.", json!({})).await?;
    core::reindex_search(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(json!({"deleted":true})))
}
async fn list(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    let scope = authz::case_scope_sql(&actor);
    let sql = format!("SELECT c.* FROM cases c WHERE {} AND c.status='draft' ORDER BY c.updated_at DESC", scope.sql);
    let cases: Vec<CaseRow> = db::bind_all_as(sqlx::query_as(&sql), &scope.binds).fetch_all(&state.db).await?;
    let mut conn = state.db.acquire().await?;
    let mut items = vec![];
    for c in cases {
        if authz::case_access(&mut conn, &actor, c.id).await? == CaseAccess::Applicant {
            items.push(super::search::summary(&mut conn, &c).await?);
        }
    }
    Ok(Json(json!({"items":items})))
}
