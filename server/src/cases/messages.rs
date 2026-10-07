use super::{
    core::{self, Visibility},
    record,
};
use crate::{
    auth::Actor,
    authz::{self, CaseAccess},
    db, deadlines,
    error::{AppError, AppResult},
    notify::{self, Notice},
    state::AppState,
    time,
    web::{Json, Path},
};
use axum::{Router, extract::State, routing::get};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cases/{id}/messages", get(messages).post(post_message))
        .route("/api/cases/{id}/notes", get(notes).post(post_note))
        .route("/api/staff/cases/{id}/notes", get(notes).post(post_note))
}
#[derive(Deserialize)]
struct Input {
    body: String,
    document_version_id: Option<i64>,
    #[serde(default)]
    requires_response: bool,
    expected_revision: Option<i64>,
}
fn validate_body(body: &str) -> AppResult<()> {
    if body.trim().is_empty() || body.len() > 20000 {
        Err(AppError::field("body", "Enter a message of 1 to 20,000 characters."))
    } else {
        Ok(())
    }
}
async fn validate_document(tx: &mut SqliteConnection, id: i64, v: Option<i64>) -> AppResult<()> {
    if let Some(v) = v {
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM document_versions v JOIN documents d ON d.id=v.document_id WHERE v.id=? AND d.case_id=? AND d.visibility='applicant' AND d.disposed_at IS NULL)").bind(v).bind(id).fetch_one(&mut *tx).await?;
        if !valid {
            return Err(AppError::field(
                "document_version_id",
                "Choose an applicant-visible document in this request.",
            ));
        }
    }
    Ok(())
}
pub async fn post_staff_message(
    tx: &mut SqliteConnection,
    actor: &Actor,
    case_id: i64,
    body: &str,
    document_version_id: Option<i64>,
    requires_response: bool,
) -> AppResult<i64> {
    post_staff_message_at(tx, actor, case_id, body, document_version_id, requires_response, crate::clock::now()).await
}
pub async fn post_staff_message_at(
    tx: &mut SqliteConnection,
    actor: &Actor,
    case_id: i64,
    body: &str,
    document_version_id: Option<i64>,
    requires_response: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> AppResult<i64> {
    let (case, access) = authz::require_staff_case(tx, actor, case_id).await?;
    if !access.can_manage() {
        return Err(AppError::forbidden());
    }
    validate_body(body)?;
    validate_document(tx, case_id, document_version_id).await?;
    if !super::workflow::is_open(&case) {
        return Err(AppError::conflict("Reopen the request before sending a message."));
    }
    if requires_response {
        let open:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM case_messages WHERE case_id=? AND requires_response=1 AND resolved_at IS NULL)").bind(case_id).fetch_one(&mut *tx).await?;
        if open {
            return Err(AppError::conflict("There is already an unanswered information request."));
        }
    }
    let id:i64=sqlx::query_scalar("INSERT INTO case_messages(case_id,author_user_id,from_staff,body,document_version_id,requires_response,created_at) VALUES (?,?,1,?,?,?,?) RETURNING id").bind(case_id).bind(actor.db_id()).bind(body.trim()).bind(document_version_id).bind(requires_response).bind(time::now_str()).fetch_one(&mut *tx).await?;
    if requires_response {
        sqlx::query("UPDATE cases SET status='waiting_on_applicant',updated_at=? WHERE id=?")
            .bind(time::now_str())
            .bind(case_id)
            .execute(&mut *tx)
            .await?;
        deadlines::api::pause_at(tx, case_id, id, body, now).await?;
    }
    record(
        tx,
        actor,
        case_id,
        "message.staff",
        Visibility::Applicant,
        if requires_response { "We need more information from you." } else { "Council sent you a message." },
        json!({"message_id":id,"requires_response":requires_response}),
    )
    .await?;
    notify::send(
        tx,
        Notice {
            user_id: case.applicant_user_id,
            email: case.applicant_email,
            phone: case.applicant_phone,
            case_id: Some(case_id),
            subject: if requires_response {
                "Your reply is needed".into()
            } else {
                "Message about your request".into()
            },
            body: body.into(),
            link: Some(format!("/my/cases/{case_id}")),
        },
    )
    .await?;
    Ok(id)
}
pub async fn applicant_reply(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    id: i64,
    body: &str,
    document_version_id: Option<i64>,
) -> AppResult<i64> {
    let (case, access) = authz::require_case(tx, actor, id).await?;
    if access != CaseAccess::Applicant {
        return Err(AppError::not_found());
    }
    if !super::workflow::is_open(&case) {
        return Err(AppError::conflict("This request is closed."));
    }
    validate_body(body)?;
    validate_document(tx, id, document_version_id).await?;
    let mid:i64=sqlx::query_scalar("INSERT INTO case_messages(case_id,author_user_id,from_staff,body,document_version_id,created_at) VALUES (?,?,0,?,?,?) RETURNING id").bind(id).bind(actor.db_id()).bind(body.trim()).bind(document_version_id).bind(time::fmt(state.now())).fetch_one(&mut *tx).await?;
    sqlx::query(
        "UPDATE case_messages SET resolved_at=? WHERE case_id=? AND requires_response=1 AND resolved_at IS NULL AND NOT EXISTS(SELECT 1 FROM document_comments c WHERE c.message_id=case_messages.id AND c.request_new_version=1 AND c.resolved_at IS NULL)",
    )
    .bind(time::fmt(state.now()))
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let outstanding: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM case_messages WHERE case_id=? AND requires_response=1 AND resolved_at IS NULL)",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if case.status == "waiting_on_applicant" && !outstanding {
        sqlx::query("UPDATE cases SET status='in_progress' WHERE id=?").bind(id).execute(&mut *tx).await?;
        deadlines::api::resume_at(tx, id, "applicant_responded", state.now()).await?;
    }
    record(
        tx,
        actor,
        id,
        "message.applicant",
        Visibility::Applicant,
        "Applicant replied to Council.",
        json!({"message_id":mid}),
    )
    .await?;
    let owners: Vec<i64> =
        sqlx::query_scalar("SELECT a.user_id FROM case_assignments a JOIN users u ON u.id=a.user_id WHERE a.case_id=? AND a.ended_at IS NULL AND u.is_active=1")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    for uid in owners {
        let Some(recipient) = Actor::load_recipient(tx, uid).await? else { continue };
        if authz::case_access(tx, &recipient, id).await?.is_staff() {
            notify::send(
                tx,
                Notice {
                    user_id: Some(uid),
                    case_id: Some(id),
                    subject: "Applicant replied".into(),
                    body: "Please review the applicant's reply.".into(),
                    link: Some(format!("/staff/cases/{id}")),
                    ..Notice::default()
                },
            )
            .await?;
        }
    }
    Ok(mid)
}
/// Resolve only messages whose replacement comments have all been satisfied.
pub async fn resolve_document_requests(tx: &mut SqliteConnection, state: &AppState, case_id: i64) -> AppResult<()> {
    let changed = sqlx::query("UPDATE case_messages SET resolved_at=? WHERE case_id=? AND requires_response=1 AND resolved_at IS NULL AND EXISTS(SELECT 1 FROM document_comments c WHERE c.message_id=case_messages.id AND c.request_new_version=1) AND NOT EXISTS(SELECT 1 FROM document_comments c WHERE c.message_id=case_messages.id AND c.request_new_version=1 AND c.resolved_at IS NULL)")
        .bind(time::fmt(state.now())).bind(case_id).execute(&mut *tx).await?.rows_affected();
    let outstanding: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM case_messages WHERE case_id=? AND requires_response=1 AND resolved_at IS NULL)",
    )
    .bind(case_id)
    .fetch_one(&mut *tx)
    .await?;
    if changed > 0 && !outstanding {
        sqlx::query("UPDATE cases SET status='in_progress' WHERE id=? AND status='waiting_on_applicant'")
            .bind(case_id)
            .execute(&mut *tx)
            .await?;
        deadlines::api::resume_at(tx, case_id, "applicant_responded", state.now()).await?;
    }
    Ok(())
}

async fn post_message(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<Input>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&state.db).await?;
    let (_, access) = authz::require_case(&mut tx, &actor, id).await?;
    if access.is_staff() && input.expected_revision.is_none() {
        return Err(AppError::field("expected_revision", "Reload the request before posting."));
    }
    core::bump_revision(&mut tx, id, input.expected_revision).await?;
    let mid = if access.is_staff() {
        post_staff_message_at(
            &mut tx,
            &actor,
            id,
            &input.body,
            input.document_version_id,
            input.requires_response,
            state.now(),
        )
        .await?
    } else {
        applicant_reply(&mut tx, &state, &actor, id, &input.body, input.document_version_id).await?
    };
    tx.commit().await?;
    Ok(Json(json!({"id":mid})))
}
pub async fn thread(tx: &mut SqliteConnection, actor: &Actor, id: i64) -> AppResult<Value> {
    let (_, access) = authz::require_case(tx, actor, id).await?;
    if matches!(access, CaseAccess::TaskOnly | CaseAccess::None) {
        return Err(AppError::not_found());
    }
    let rows:Vec<MessageProjectionRow>=sqlx::query_as("SELECT m.id,m.body,m.from_staff,m.requires_response,m.created_at,m.resolved_at,m.document_version_id,u.display_name FROM case_messages m LEFT JOIN users u ON u.id=m.author_user_id WHERE case_id=? ORDER BY m.id").bind(id).fetch_all(&mut *tx).await?;
    Ok(
        json!({"items":rows.into_iter().map(|(id,body,from_staff,requires_response,created_at,resolved_at,document_version_id,author)|json!({"id":id,"body":body,"from_staff":from_staff,"requires_response":requires_response,"created_at":created_at,"resolved_at":resolved_at,"document_version_id":document_version_id,"author_name":author})).collect::<Vec<_>>()}),
    )
}
async fn messages(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut tx = state.db.acquire().await?;
    Ok(Json(thread(&mut tx, &actor, id).await?))
}
async fn notes(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut tx = state.db.acquire().await?;
    authz::require_staff_case(&mut tx, &actor, id).await?;
    let rows:Vec<(i64,String,String,String)>=sqlx::query_as("SELECT n.id,n.body,n.created_at,u.display_name FROM internal_notes n JOIN users u ON u.id=n.author_user_id WHERE n.case_id=? ORDER BY n.id").bind(id).fetch_all(&mut *tx).await?;
    Ok(Json(
        json!({"items":rows.into_iter().map(|(id,body,created_at,author)|json!({"id":id,"body":body,"created_at":created_at,"author_name":author})).collect::<Vec<_>>()}),
    ))
}
async fn post_note(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<Input>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&state.db).await?;
    let (_, access) = authz::require_staff_case(&mut tx, &actor, id).await?;
    if !access.can_manage() {
        return Err(AppError::forbidden());
    }
    validate_body(&input.body)?;
    let revision = input
        .expected_revision
        .ok_or_else(|| AppError::field("expected_revision", "Reload the request before posting."))?;
    core::bump_revision(&mut tx, id, Some(revision)).await?;
    let nid: i64 = sqlx::query_scalar(
        "INSERT INTO internal_notes(case_id,author_user_id,body,created_at) VALUES (?,?,?,?) RETURNING id",
    )
    .bind(id)
    .bind(actor.user_id)
    .bind(input.body.trim())
    .bind(time::fmt(state.now()))
    .fetch_one(&mut *tx)
    .await?;
    record(&mut tx, &actor, id, "note.created", Visibility::Staff, "Internal note recorded.", json!({"note_id":nid}))
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":nid})))
}

type MessageProjectionRow = (i64, String, bool, bool, String, Option<String>, Option<i64>, Option<String>);
