use crate::{
    auth::Actor,
    authz::CaseAccess,
    cases::core::Visibility,
    db::write_tx,
    error::{AppError, AppResult},
    state::AppState,
    storage, time,
    web::{Json, Path},
};
use axum::{
    extract::{FromRequest, Multipart, Request, State},
    http::header,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

pub struct DocsMultipart(Multipart);
impl FromRequest<AppState> for DocsMultipart {
    type Rejection = AppError;
    async fn from_request(req: Request, state: &AppState) -> AppResult<Self> {
        Multipart::from_request(req, state)
            .await
            .map(Self)
            .map_err(|_| AppError::field("file", "Send a valid multipart file upload."))
    }
}
async fn revision(
    tx: &mut SqliteConnection,
    case: i64,
    access: CaseAccess,
    fields: &std::collections::HashMap<String, String>,
) -> AppResult<()> {
    let expected = fields
        .get("expected_revision")
        .map(|s| s.parse::<i64>())
        .transpose()
        .map_err(|_| AppError::field("expected_revision", "Reload the case and try again."))?;
    if access.is_staff() && expected.is_none() {
        return Err(AppError::field("expected_revision", "Reload the case and try again."));
    }
    crate::cases::core::bump_revision(tx, case, expected).await?;
    Ok(())
}
#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct Document {
    pub id: i64,
    pub case_id: i64,
    pub category: String,
    pub title: String,
    pub visibility: String,
    pub requirement_key: Option<String>,
    #[sqlx(skip)]
    pub versions: Vec<Version>,
}
#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct Version {
    pub id: i64,
    pub version: i64,
    pub uploaded_at: String,
    pub uploader: String,
    pub note: Option<String>,
    #[sqlx(skip)]
    pub comments: Vec<Comment>,
}
#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct Comment {
    pub id: i64,
    pub body: String,
    pub visibility: String,
    pub author: String,
    pub created_at: String,
    pub resolved_by_version_id: Option<i64>,
    pub request_new_version: bool,
}

pub async fn list(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Vec<Document>>> {
    let mut conn = state.db.acquire().await?;
    let (_, a) = super::access(&mut conn, &actor, id).await?;
    Ok(Json(project(&mut conn, id, a).await?))
}
pub(crate) async fn project(tx: &mut SqliteConnection, id: i64, a: CaseAccess) -> AppResult<Vec<Document>> {
    let staff = a.is_staff();
    let mut docs:Vec<Document>=sqlx::query_as("SELECT id,case_id,category,title,visibility,requirement_key FROM documents WHERE case_id=? AND disposed_at IS NULL AND (? OR visibility='applicant') ORDER BY category,id").bind(id).bind(staff).fetch_all(&mut *tx).await?;
    for doc in &mut docs {
        doc.versions=sqlx::query_as("SELECT v.id,v.version,v.uploaded_at,COALESCE(u.display_name,'ServiceHub') uploader,v.note FROM document_versions v LEFT JOIN users u ON u.id=v.uploaded_by WHERE document_id=? ORDER BY version").bind(doc.id).fetch_all(&mut *tx).await?;
        for v in &mut doc.versions {
            v.comments=sqlx::query_as("SELECT c.id,c.body,c.visibility,u.display_name author,c.created_at,c.resolved_by_version_id,c.request_new_version FROM document_comments c JOIN users u ON u.id=c.author_user_id WHERE document_version_id=? AND (? OR visibility='applicant') ORDER BY c.id").bind(v.id).bind(staff).fetch_all(&mut *tx).await?;
        }
    }
    Ok(docs)
}
pub(crate) fn writable(a: CaseAccess) -> AppResult<()> {
    if a == CaseAccess::Applicant || a.can_manage() { Ok(()) } else { Err(AppError::forbidden()) }
}
struct Upload {
    bytes: Vec<u8>,
    name: String,
    fields: std::collections::HashMap<String, String>,
    resolves: Vec<i64>,
}
async fn parse(mut multi: Multipart) -> AppResult<Upload> {
    let mut out = Upload { bytes: vec![], name: String::new(), fields: Default::default(), resolves: vec![] };
    let mut file_seen = false;
    while let Some(f) = multi.next_field().await.map_err(|_| AppError::field("file", "Invalid multipart upload."))? {
        let key = f.name().unwrap_or("").to_string();
        if key == "file" {
            if file_seen {
                return Err(AppError::field("file", "Upload one file at a time."));
            }
            file_seen = true;
            out.name = f.file_name().unwrap_or("upload").into();
            out.bytes = f.bytes().await.map_err(|_| AppError::field("file", "Could not read the file."))?.to_vec();
        } else {
            let value = f.text().await.map_err(|_| AppError::validation_msg("Invalid upload field."))?;
            if value.len() > 20000 {
                return Err(AppError::field(&key, "This field is too long."));
            }
            if key == "resolves_comment_ids[]" {
                out.resolves.push(value.parse().map_err(|_| AppError::field(&key, "Choose a document comment."))?);
            } else if key == "resolves_comment_ids" {
                out.resolves =
                    serde_json::from_str(&value).map_err(|_| AppError::field(&key, "Choose document comments."))?;
            } else {
                out.fields.insert(key, value);
            }
        }
    }
    Ok(out)
}
pub(crate) fn editable(case: &crate::cases::core::CaseRow) -> AppResult<()> {
    if matches!(case.status.as_str(), "completed" | "refused" | "withdrawn" | "cancelled" | "closed_duplicate") {
        return Err(AppError::conflict("This case is closed."));
    }
    Ok(())
}
pub async fn upload(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    multi: DocsMultipart,
) -> AppResult<Json<serde_json::Value>> {
    {
        let mut c = state.db.acquire().await?;
        let (case, a) = super::access(&mut c, &actor, id).await?;
        writable(a)?;
        editable(&case)?;
    }
    let u = parse(multi.0).await?;
    let title = u.fields.get("title").cloned().unwrap_or(u.name.clone());
    super::text("title", &title, 200)?;
    let category = u.fields.get("category").map(String::as_str).unwrap_or("application");
    super::text("category", category, 60)?;
    if generated_category(category) {
        return Err(AppError::field("category", "Issued results are created by the decision or letter process."));
    }
    let visibility = match u.fields.get("visibility").map(String::as_str).unwrap_or("applicant") {
        "applicant" => Visibility::Applicant,
        "staff" => Visibility::Staff,
        _ => return Err(AppError::field("visibility", "Choose applicant or staff.")),
    };
    let staged = storage::stage(&state, &u.bytes, &u.name, storage::AllowList::Docs).await?;
    let mut tx = write_tx(&state.db).await?;
    let (case, a) = super::access(&mut tx, &actor, id).await?;
    editable(&case)?;
    writable(a)?;
    if visibility == Visibility::Staff && !a.is_staff() {
        return Err(AppError::forbidden());
    }
    revision(&mut tx, id, a, &u.fields).await?;
    let requirement = u.fields.get("requirement_key").map(String::as_str).filter(|s| !s.is_empty());
    if let Some(key) = requirement {
        let definition: String = sqlx::query_scalar("SELECT definition_json FROM service_versions WHERE id=?")
            .bind(case.service_version_id)
            .fetch_one(&mut *tx)
            .await?;
        let definition: serde_json::Value = serde_json::from_str(&definition)?;
        if !definition
            .get("documents")
            .and_then(|v| v.as_array())
            .is_some_and(|docs| docs.iter().any(|d| d.get("key").and_then(|v| v.as_str()) == Some(key)))
        {
            return Err(AppError::field("requirement_key", "Choose a document requirement from this request."));
        }
    }
    validate_upload_category(&mut tx, &case, a, category).await?;
    upload_quota(&mut tx, actor.user_id, staged.size_bytes).await?;
    let blob = storage::register(&mut tx, staged, actor.db_id()).await?;
    let (doc, vid) =
        super::api::insert(&mut tx, id, category, &title, visibility, requirement, blob.id, actor.db_id()).await?;
    super::changed(
        &mut tx,
        actor.db_id(),
        id,
        "documents.upload",
        visibility,
        &format!("Uploaded {title}, version 1."),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({"id":doc,"version_id":vid})))
}
pub(crate) async fn document_access(
    tx: &mut SqliteConnection,
    actor: &Actor,
    id: i64,
) -> AppResult<(crate::cases::core::CaseRow, CaseAccess, String, String)> {
    let (case_id, visibility, title, category): (i64, String, String, String) =
        sqlx::query_as("SELECT case_id,visibility,title,category FROM documents WHERE id=? AND disposed_at IS NULL")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(AppError::not_found)?;
    let (case, a) = super::access(tx, actor, case_id).await?;
    if visibility == "staff" && !a.is_staff() {
        return Err(AppError::not_found());
    }
    Ok((case, a, title, category))
}
pub(crate) async fn version_access(
    tx: &mut SqliteConnection,
    actor: &Actor,
    vid: i64,
) -> AppResult<(crate::cases::core::CaseRow, CaseAccess, i64, i64, String)> {
    let (doc, blob): (i64, i64) = sqlx::query_as("SELECT document_id,blob_id FROM document_versions WHERE id=?")
        .bind(vid)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    let (case, a, title, _) = document_access(tx, actor, doc).await?;
    Ok((case, a, doc, blob, title))
}
pub async fn version(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    multi: DocsMultipart,
) -> AppResult<Json<serde_json::Value>> {
    {
        let mut c = state.db.acquire().await?;
        let (case, a, _, category) = document_access(&mut c, &actor, id).await?;
        editable(&case)?;
        writable(a)?;
        immutable(&mut c, id, &category).await?;
    }
    let u = parse(multi.0).await?;
    let staged = storage::stage(&state, &u.bytes, &u.name, storage::AllowList::Docs).await?;
    let mut tx = write_tx(&state.db).await?;
    let (case, a, title, category) = document_access(&mut tx, &actor, id).await?;
    editable(&case)?;
    writable(a)?;
    immutable(&mut tx, id, &category).await?;
    revision(&mut tx, case.id, a, &u.fields).await?;
    let n: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version),0)+1 FROM document_versions WHERE document_id=?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    upload_quota(&mut tx, actor.user_id, staged.size_bytes).await?;
    let blob = storage::register(&mut tx, staged, actor.db_id()).await?;
    let vid:i64=sqlx::query_scalar("INSERT INTO document_versions(document_id,version,blob_id,uploaded_by,note,uploaded_at) VALUES(?,?,?,?,?,?) RETURNING id").bind(id).bind(n).bind(blob.id).bind(actor.db_id()).bind(u.fields.get("note")).bind(time::now_str()).fetch_one(&mut *tx).await?;
    for cid in u.resolves {
        let updated=sqlx::query("UPDATE document_comments SET resolved_at=?,resolved_by_version_id=? WHERE id=? AND resolved_at IS NULL AND visibility='applicant' AND document_version_id IN (SELECT id FROM document_versions WHERE document_id=? AND version<?)").bind(time::now_str()).bind(vid).bind(cid).bind(id).bind(n).execute(&mut *tx).await?;
        if updated.rows_affected() != 1 {
            return Err(AppError::field(
                "resolves_comment_ids",
                "Choose unresolved comments on an earlier version of this document.",
            ));
        }
    }
    crate::cases::messages::resolve_document_requests(&mut tx, &state, case.id).await?;
    let visibility: String =
        sqlx::query_scalar("SELECT visibility FROM documents WHERE id=?").bind(id).fetch_one(&mut *tx).await?;
    let visibility = if visibility == "staff" { Visibility::Staff } else { Visibility::Applicant };
    let recipients: Vec<i64> = sqlx::query_scalar("SELECT DISTINCT author_user_id FROM document_comments WHERE resolved_by_version_id=? AND request_new_version=1")
        .bind(vid).fetch_all(&mut *tx).await?;
    for user in recipients {
        crate::notify::send(
            &mut tx,
            crate::notify::Notice {
                user_id: Some(user),
                email: None,
                phone: None,
                case_id: Some(case.id),
                subject: format!("Replacement received: {title} v{n}"),
                body: "Review the new version against your document comments.".into(),
                link: Some(format!("/staff/cases/{}?tab=documents.documents", case.id)),
            },
        )
        .await?;
    }
    super::changed(
        &mut tx,
        actor.db_id(),
        case.id,
        "documents.new_version",
        visibility,
        &format!("Uploaded {title}, version {n}; earlier versions remain available."),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({"version_id":vid,"version":n})))
}
pub async fn download(State(state): State<AppState>, actor: Actor, Path(vid): Path<i64>) -> AppResult<Response> {
    let mut c = state.db.acquire().await?;
    let (_, _, _, blob, _) = version_access(&mut c, &actor, vid).await?;
    drop(c);
    blob_response(&state, blob, false).await
}
pub(crate) async fn blob_response(state: &AppState, blob: i64, inline: bool) -> AppResult<Response> {
    let (row, bytes) = storage::read(state, blob).await?;
    let name: String = row
        .original_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    Ok((
        [
            (header::CONTENT_TYPE, row.mime),
            (
                header::CONTENT_DISPOSITION,
                format!("{}; filename=\"{}\"", if inline { "inline" } else { "attachment" }, name),
            ),
            (header::CACHE_CONTROL, "private, no-store".into()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
        ],
        bytes,
    )
        .into_response())
}
#[derive(Deserialize)]
pub struct CommentInput {
    expected_revision: i64,
    body: String,
    visibility: String,
    #[serde(default)]
    request_new_version: bool,
}
pub async fn comment(
    State(state): State<AppState>,
    actor: Actor,
    Path(vid): Path<i64>,
    Json(input): Json<CommentInput>,
) -> AppResult<Json<serde_json::Value>> {
    super::text("body", &input.body, 5000)?;
    if !matches!(input.visibility.as_str(), "applicant" | "internal") {
        return Err(AppError::field("visibility", "Choose applicant or internal."));
    }
    if input.request_new_version && input.visibility != "applicant" {
        return Err(AppError::field("visibility", "A replacement request must be visible to the applicant."));
    }
    let mut tx = write_tx(&state.db).await?;
    let (case, _, doc, _, title) = version_access(&mut tx, &actor, vid).await?;
    editable(&case)?;
    super::manage(
        &mut tx,
        &actor,
        case.id,
        &[
            crate::authz::Role::Intake,
            crate::authz::Role::Specialist,
            crate::authz::Role::Manager,
            crate::authz::Role::ComplaintsOfficer,
        ],
    )
    .await?;
    let visibility: String =
        sqlx::query_scalar("SELECT visibility FROM documents WHERE id=?").bind(doc).fetch_one(&mut *tx).await?;
    if visibility == "staff" && input.visibility == "applicant" {
        return Err(AppError::field("visibility", "Comments on internal documents must remain internal."));
    }
    crate::cases::core::bump_revision(&mut tx, case.id, Some(input.expected_revision)).await?;
    let message = if input.request_new_version {
        Some(
            crate::cases::messages::post_staff_message_at(
                &mut tx,
                &actor,
                case.id,
                &format!("Replace {title}: {}", input.body),
                Some(vid),
                true,
                state.now(),
            )
            .await?,
        )
    } else {
        None
    };
    let id:i64=sqlx::query_scalar("INSERT INTO document_comments(document_version_id,author_user_id,visibility,body,created_at,request_new_version,message_id) VALUES(?,?,?,?,?,?,?) RETURNING id").bind(vid).bind(actor.user_id).bind(&input.visibility).bind(&input.body).bind(time::now_str()).bind(input.request_new_version).bind(message).fetch_one(&mut *tx).await?;
    super::changed(
        &mut tx,
        actor.db_id(),
        case.id,
        "documents.comment",
        if input.visibility == "internal" { Visibility::Staff } else { Visibility::Applicant },
        &format!("Comment added to {title}."),
    )
    .await?;
    if input.visibility == "applicant" && !input.request_new_version {
        super::notify_applicant(&mut tx, &case, "A document has a new comment", &title).await?;
    }
    tx.commit().await?;
    Ok(Json(serde_json::json!({"id":id})))
}

fn generated_category(category: &str) -> bool {
    matches!(category, "decision" | "letter" | "certificate" | "invoice" | "credit_note" | "booking_confirmation")
}
async fn immutable(tx: &mut SqliteConnection, id: i64, category: &str) -> AppResult<()> {
    let generated: bool =
        sqlx::query_scalar("SELECT generated FROM documents WHERE id=?").bind(id).fetch_one(tx).await?;
    if generated || generated_category(category) {
        return Err(AppError::conflict("Generated documents are immutable."));
    }
    Ok(())
}
async fn validate_upload_category(
    tx: &mut SqliteConnection,
    case: &crate::cases::core::CaseRow,
    access: CaseAccess,
    category: &str,
) -> AppResult<()> {
    if generated_category(category) {
        return Err(AppError::field("category", "Generated document categories are reserved."));
    }
    if access == CaseAccess::Applicant && !matches!(category, "application" | "supporting" | "receipt") {
        let definition: String = sqlx::query_scalar("SELECT definition_json FROM service_versions WHERE id=?")
            .bind(case.service_version_id)
            .fetch_one(tx)
            .await?;
        let definition: serde_json::Value = serde_json::from_str(&definition)?;
        if !definition["documents"].as_array().is_some_and(|ds| ds.iter().any(|d| d["key"].as_str() == Some(category)))
        {
            return Err(AppError::field("category", "Choose an application document category."));
        }
    }
    Ok(())
}
async fn upload_quota(tx: &mut SqliteConnection, user: i64, size: i64) -> AppResult<()> {
    let used: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(b.size_bytes),0) FROM document_versions v JOIN blobs b ON b.id=v.blob_id JOIN documents d ON d.id=v.document_id WHERE v.uploaded_by=? AND d.generated=0 AND d.disposed_at IS NULL")
        .bind(user).fetch_one(tx).await?;
    if used.saturating_add(size) > 100 * 1024 * 1024 {
        return Err(AppError::field("file", "Your document uploads exceed the 100 MB limit."));
    }
    Ok(())
}
