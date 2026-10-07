use super::admin::{self, NewService};
use super::upload::Multipart;
use crate::{
    auth::Actor,
    db,
    error::{AppError, AppResult},
    state::AppState,
    storage::{self, AllowList},
    web::Json,
};
use axum::{Router, extract::State, routing::post};
use serde_json::{Value, json};
pub fn routes() -> Router<AppState> {
    Router::new().route("/api/admin/services/ai-suggest", post(suggest))
}
async fn suggest(State(state): State<AppState>, actor: Actor, mut form: Multipart) -> AppResult<Json<Value>> {
    if !state.cfg.ai_enabled {
        return Err(AppError::not_found());
    }
    admin::require_admin(&actor)?;
    let mut service_id = None;
    let mut slug = None;
    let mut name = None;
    let mut file = None;
    while let Some(f) = form.next_field().await.map_err(|_| AppError::field("file", "Cannot read uploaded form."))? {
        match f.name() {
            Some("file") => {
                let filename = f.file_name().unwrap_or("council-form.pdf").to_owned();
                file = Some((
                    filename,
                    f.bytes().await.map_err(|_| AppError::field("file", "Cannot read uploaded form."))?.to_vec(),
                ));
            }
            Some("service_id") => service_id = f.text().await.ok().and_then(|s| s.parse::<i64>().ok()),
            Some("slug") => slug = f.text().await.ok(),
            Some("name") => name = f.text().await.ok(),
            _ => {}
        }
    }
    let (filename, bytes) = file.ok_or_else(|| AppError::field("file", "Choose a council PDF form."))?;
    let staged = storage::stage(&state, &bytes, &filename, AllowList::Docs).await?;
    if staged.mime != "application/pdf" {
        return Err(AppError::field("file", "Choose a PDF form."));
    }
    let path = storage::blob_path(&state.cfg.blobs_dir(), &staged.sha256);
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new("pdftotext").kill_on_drop(true).arg("-layout").arg(&path).arg("-").output(),
    )
    .await
    .map_err(|_| AppError::field("file", "PDF text extraction took too long."))?
    .map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::validation_msg(
                "PDF extraction is unavailable. Install pdftotext (poppler-utils), or build the draft manually.",
            )
        } else {
            AppError::from(e)
        }
    })?;
    if !output.status.success() {
        return Err(AppError::field("file", "Cannot extract text from this PDF."));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if text.trim().is_empty() {
        return Err(AppError::field(
            "file",
            "This PDF has no extractable text. Use a text PDF or build the draft manually.",
        ));
    }
    let response = state
        .http
        .post(format!("{}/mock/ai/suggest", state.cfg.internal_base_url))
        .header("X-Mock-Key", &state.cfg.mock_api_key)
        .json(&json!({"text":text}))
        .send()
        .await
        .map_err(|_| AppError::conflict("The mock AI helper is unavailable. You can build the draft manually."))?;
    if !response.status().is_success() {
        return Err(AppError::conflict("The mock AI helper could not read this form."));
    }
    let suggestion: Value = response.json().await.map_err(|e| AppError::internal(format!("Mock AI response: {e}")))?;
    let mut tx = db::write_tx(&state.db).await?;
    let blob = storage::register(&mut tx, staged, actor.db_id()).await?;
    let id = if let Some(id) = service_id {
        id
    } else {
        admin::create_service(
            &mut tx,
            &actor,
            &NewService {
                slug: slug.ok_or_else(|| AppError::field("slug", "Enter the new service slug."))?,
                name: name.ok_or_else(|| AppError::field("name", "Enter the new service name."))?,
                category: "Customer Care".into(),
                module: "generic".into(),
                department: "Customer Care".into(),
            },
        )
        .await?
    };
    let note = format!("AI-suggested from {filename}; every rule must be checked");
    let vid = admin::create_version(&mut tx, &actor, id, suggestion["definition"].clone(), Some(&note)).await?;
    sqlx::query("UPDATE service_versions SET source_blob_id=? WHERE id=?")
        .bind(blob.id)
        .bind(vid)
        .execute(&mut *tx)
        .await?;
    admin::audit(&mut tx, &actor, id, "service.ai_suggested", json!({"version_id":vid,"source_blob_id":blob.id}))
        .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"service_id":id,"version_id":vid,"definition":suggestion["definition"],"warnings":suggestion["warnings"],"source_note":note}),
    ))
}
