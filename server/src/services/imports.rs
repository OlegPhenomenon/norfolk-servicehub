use super::admin::{self, NewService};
use super::upload::Multipart;
use crate::{
    auth::Actor,
    db,
    error::{AppError, AppResult},
    state::AppState,
    storage::{self, AllowList},
    time,
    web::{Json, Path},
};
use axum::{
    Router,
    extract::State,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/service-imports", post(upload).get(list))
        .route("/api/admin/service-imports/{id}", get(detail))
        .route("/api/admin/service-imports/{id}/apply", post(apply))
}
#[derive(Deserialize, Serialize)]
struct Item {
    slug: String,
    name: String,
    category: String,
    module: String,
    department: String,
    source_url: Option<String>,
    source_file: Option<String>,
    definition: Value,
}
pub async fn uploaded_file(mut form: Multipart) -> AppResult<(String, Vec<u8>)> {
    while let Some(field) =
        form.next_field().await.map_err(|_| AppError::field("file", "Cannot read uploaded file."))?
    {
        if field.name() == Some("file") {
            let name = field.file_name().unwrap_or("import.json").to_owned();
            let bytes = field.bytes().await.map_err(|_| AppError::field("file", "Cannot read uploaded file."))?;
            return Ok((name, bytes.to_vec()));
        }
    }
    Err(AppError::field("file", "Choose a file to upload."))
}
async fn upload(State(state): State<AppState>, actor: Actor, mut form: Multipart) -> AppResult<Json<Value>> {
    admin::require_admin(&actor)?;
    let mut main = None;
    let mut sources = std::collections::BTreeMap::new();
    while let Some(field) = form.next_field().await.map_err(|_| AppError::field("file", "Cannot read upload."))? {
        let key = field.name().unwrap_or("").to_owned();
        let name = field.file_name().unwrap_or("import.json").to_owned();
        let bytes = field.bytes().await.map_err(|_| AppError::field("file", "Cannot read file."))?.to_vec();
        if key == "file" {
            main = Some((name, bytes));
        } else if key == "source" {
            let staged = storage::stage(&state, &bytes, &name, AllowList::Docs).await?;
            if staged.mime != "application/pdf" {
                return Err(AppError::field("source", "Original forms must be PDFs."));
            }
            if sources.insert(name, staged).is_some() {
                return Err(AppError::field("source", "Use unique source filenames."));
            }
        }
    }
    let (name, bytes) = main.ok_or_else(|| AppError::field("file", "Choose the import JSON."))?;
    let raw: Vec<Value> = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::field("file", "The JSON file must contain an array of service definitions."))?;
    if raw.is_empty() || raw.len() > 100 {
        return Err(AppError::field("file", "Import 1 to 100 services at a time."));
    }
    let staged = storage::stage(&state, &bytes, &name, AllowList::Data).await?;
    let mut tx = db::write_tx(&state.db).await?;
    let blob = storage::register(&mut tx, staged, actor.db_id()).await?;
    let mut source_ids = std::collections::BTreeMap::new();
    for (name, staged) in sources {
        source_ids.insert(name, storage::register(&mut tx, staged, actor.db_id()).await?.id);
    }
    let mut report = vec![];
    let mut slugs = std::collections::HashSet::new();
    for (index, item) in raw.into_iter().enumerate() {
        let mut issues = match serde_json::from_value::<Item>(item.clone()) {
            Ok(i) => admin::issues(&mut tx, &i.definition, &i.module, &i.slug).await?,
            Err(e) => json!([{"path":"item","message":e.to_string()}]),
        };
        if let Some(slug) = item["slug"].as_str()
            && (!slugs.insert(slug.to_owned())
                || slug.is_empty()
                || !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
        {
            issues.as_array_mut().expect("array").push(json!({"path":"slug","message":"Use unique lowercase slugs."}));
        }
        for key in ["name", "category", "department"] {
            if item[key].as_str().is_none_or(|s| s.trim().is_empty()) {
                issues.as_array_mut().expect("array").push(json!({"path":key,"message":"This field is required."}));
            }
        }
        let source_blob_id = item["source_file"].as_str().and_then(|name| source_ids.get(name)).copied();
        if item["source_file"].as_str().is_some() && source_blob_id.is_none() {
            issues
                .as_array_mut()
                .expect("issues")
                .push(json!({"path":"source_file","message":"Attach the original PDF named in source_file."}));
        }
        report.push(
            json!({"index":index,"item":item,"source_blob_id":source_blob_id,"valid":issues.as_array().is_some_and(Vec::is_empty),"issues":issues}),
        );
    }
    let status = if report.iter().all(|r| r["valid"] == true) { "validated" } else { "has_errors" };
    let report = json!({"items":report});
    let id:i64=sqlx::query_scalar("INSERT INTO service_imports(uploaded_by,filename,blob_id,status,report_json,created_at) VALUES (?,?,?,?,?,?) RETURNING id").bind(actor.user_id).bind(name).bind(blob.id).bind(status).bind(report.to_string()).bind(time::fmt(state.now())).fetch_one(&mut *tx).await?;
    for (filename, blob_id) in source_ids {
        sqlx::query("INSERT INTO service_import_sources(import_id,filename,blob_id) VALUES(?,?,?)")
            .bind(id)
            .bind(filename)
            .bind(blob_id)
            .execute(&mut *tx)
            .await?;
    }
    crate::audit::record(
        &mut tx,
        actor.db_id(),
        "service_import.validated",
        "service_import",
        Some(id),
        json!({"status":status}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id,"status":status,"report":report})))
}
async fn apply(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    admin::require_admin(&actor)?;
    let mut tx = db::write_tx(&state.db).await?;
    let (status, raw): (String, String) = sqlx::query_as("SELECT status,report_json FROM service_imports WHERE id=?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let mut report: Value = serde_json::from_str(&raw)?;
    if status == "applied" {
        return Ok(Json(json!({"id":id,"status":status,"report":report})));
    }
    for row in report["items"]
        .as_array_mut()
        .ok_or_else(|| AppError::internal("Invalid import report"))?
        .iter_mut()
        .filter(|r| r["valid"] == true)
    {
        let item: Item = serde_json::from_value(row["item"].clone())?;
        let issues = admin::issues(&mut tx, &item.definition, &item.module, &item.slug).await?;
        if !issues.as_array().is_some_and(Vec::is_empty) {
            row["valid"] = json!(false);
            row["issues"] = issues;
            continue;
        }
        let existing: Option<(i64, String)> = sqlx::query_as("SELECT id,module FROM services WHERE slug=?")
            .bind(&item.slug)
            .fetch_optional(&mut *tx)
            .await?;
        let service_id = match existing {
            Some((id, module)) if module == item.module => id,
            Some(_) => {
                row["valid"] = json!(false);
                row["issues"] = json!([{"path":"module","message":"An existing service uses a different module."}]);
                continue;
            }
            None => {
                admin::create_service(
                    &mut tx,
                    &actor,
                    &NewService {
                        slug: item.slug,
                        name: item.name,
                        category: item.category,
                        module: item.module,
                        department: item.department,
                    },
                )
                .await?
            }
        };
        let v = admin::create_version(&mut tx, &actor, service_id, item.definition, item.source_url.as_deref()).await?;
        if let Some(blob) = row["source_blob_id"].as_i64() {
            sqlx::query("UPDATE service_versions SET source_blob_id=? WHERE id=?")
                .bind(blob)
                .bind(v)
                .execute(&mut *tx)
                .await?;
        } else if item.source_file.is_some() {
            return Err(AppError::conflict("The original form is missing."));
        }
        row["service_id"] = json!(service_id);
        row["version_id"] = json!(v);
        row["link"] = json!(format!("/admin/services/{service_id}"));
    }
    sqlx::query("UPDATE service_imports SET status='applied',report_json=? WHERE id=?")
        .bind(report.to_string())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    crate::audit::record(
        &mut tx,
        actor.db_id(),
        "service_import.applied",
        "service_import",
        Some(id),
        json!({"drafts_only":true}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id,"status":"applied","report":report})))
}
async fn detail(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    admin::require_admin(&actor)?;
    let (status, raw, name): (String, String, String) =
        sqlx::query_as("SELECT status,report_json,filename FROM service_imports WHERE id=?")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    Ok(Json(json!({"id":id,"status":status,"filename":name,"report":serde_json::from_str::<Value>(&raw)?})))
}
async fn list(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    admin::require_admin(&actor)?;
    let rows: Vec<(i64, String, String, String)> =
        sqlx::query_as("SELECT id,filename,status,created_at FROM service_imports ORDER BY id DESC LIMIT 100")
            .fetch_all(&state.db)
            .await?;
    Ok(Json(
        json!({"items":rows.into_iter().map(|(id,filename,status,created_at)|json!({"id":id,"filename":filename,"status":status,"created_at":created_at})).collect::<Vec<_>>()}),
    ))
}
