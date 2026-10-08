use super::{
    catalog::{self, ServiceRow},
    definition::ServiceDefinition,
    validation,
};
use crate::{
    auth::Actor,
    authz::Role,
    db,
    error::{AppError, AppResult},
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
        .route("/api/admin/services/capabilities/{module}", get(capabilities))
        .route("/api/admin/services", get(list).post(create))
        .route("/api/admin/services/{id}", get(detail).put(update_details))
        .route("/api/admin/services/{id}/versions", post(new_version))
        .route("/api/admin/services/{id}/versions/{v}", put(save))
        .route("/api/admin/services/{id}/versions/{v}/source", get(source))
        .route("/api/admin/services/{id}/versions/{v}/source-file", post(source_file))
        .route("/api/admin/services/{id}/versions/{v}/{action}", post(action))
}
pub fn require_admin(actor: &Actor) -> AppResult<()> {
    actor.require_any_role(&[Role::Sysadmin])
}
#[derive(Clone, Deserialize)]
pub struct NewService {
    pub slug: String,
    pub name: String,
    pub category: String,
    pub module: String,
    pub department: String,
}
pub async fn create_service(tx: &mut SqliteConnection, actor: &Actor, input: &NewService) -> AppResult<i64> {
    require_admin(actor)?;
    if input.slug.is_empty()
        || input.slug.len() > 100
        || !input.slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(AppError::field("slug", "Use lowercase letters, numbers and hyphens."));
    }
    if input.name.trim().is_empty() || input.category.trim().is_empty() || input.department.trim().is_empty() {
        return Err(AppError::validation_msg("Name, category and department are required."));
    }
    if !validation::MODULES.contains(&input.module.as_str()) {
        return Err(AppError::field("module", "Choose a registered module."));
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO services(slug,name,category,module,department,created_at) VALUES (?,?,?,?,?,?) RETURNING id",
    )
    .bind(&input.slug)
    .bind(&input.name)
    .bind(&input.category)
    .bind(&input.module)
    .bind(&input.department)
    .bind(time::now_str())
    .fetch_one(&mut *tx)
    .await?;
    audit(tx, actor, id, "service.created", json!({"name":input.name})).await?;
    Ok(id)
}
pub async fn create_version(
    tx: &mut SqliteConnection,
    actor: &Actor,
    id: i64,
    definition: Value,
    source_note: Option<&str>,
) -> AppResult<i64> {
    require_admin(actor)?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM services WHERE id=?)").bind(id).fetch_one(&mut *tx).await?;
    if !exists {
        return Err(AppError::not_found());
    }
    let next: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version),0)+1 FROM service_versions WHERE service_id=?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let vid:i64=sqlx::query_scalar("INSERT INTO service_versions(service_id,version,status,definition_json,source_note,created_by,created_at) VALUES (?,?,'draft',?,?,?,?) RETURNING id").bind(id).bind(next).bind(definition.to_string()).bind(source_note).bind(actor.db_id()).bind(time::now_str()).fetch_one(&mut *tx).await?;
    audit(tx, actor, id, "service.version_created", json!({"version_id":vid})).await?;
    Ok(vid)
}
pub fn blank_definition(module: &str) -> Value {
    json!({"module":module,"summary":"","outcome":"","who_can_apply":"","price_note":"","keywords":[],"fields":[],"documents":[],"workflow":{"steps":[{"key":"intake","kind":"review","role":"intake","label":"Check request","applicant_label":"We are checking your request."},{"key":"done","kind":"complete","label":"Completed","applicant_label":"Completed."}]},"deadlines":[],"pricing":[]})
}
pub async fn audit(tx: &mut SqliteConnection, actor: &Actor, id: i64, kind: &str, data: Value) -> AppResult<()> {
    crate::audit::record(tx, actor.db_id(), kind, "service", Some(id), data).await
}
async fn list(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    require_admin(&actor)?;
    let rows: Vec<ServiceRow> = sqlx::query_as("SELECT * FROM services ORDER BY name").fetch_all(&state.db).await?;
    Ok(Json(json!({"items":rows})))
}
async fn create(State(state): State<AppState>, actor: Actor, Json(input): Json<NewService>) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&state.db).await?;
    let id = create_service(&mut tx, &actor, &input).await?;
    let version = create_version(&mut tx, &actor, id, blank_definition(&input.module), None).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id,"version_id":version})))
}
async fn detail(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    require_admin(&actor)?;
    let service: ServiceRow = sqlx::query_as("SELECT * FROM services WHERE id=?").bind(id).fetch_one(&state.db).await?;
    let versions:Vec<VersionProjectionRow>=sqlx::query_as("SELECT id,version,status,definition_json,source_blob_id,source_note,created_at,published_at FROM service_versions WHERE service_id=? ORDER BY version DESC").bind(id).fetch_all(&state.db).await?;
    Ok(Json(
        json!({"service":service,"versions":versions.into_iter().map(|(id,version,status,def,source_blob_id,source_note,created_at,published_at)|json!({"id":id,"version":version,"status":status,"definition":serde_json::from_str::<Value>(&def).unwrap_or(Value::Null),"source_blob_id":source_blob_id,"source_note":source_note,"created_at":created_at,"published_at":published_at})).collect::<Vec<_>>()}),
    ))
}
async fn update_details(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<NewService>,
) -> AppResult<Json<Value>> {
    require_admin(&actor)?;
    if input.name.trim().is_empty() || input.category.trim().is_empty() || input.department.trim().is_empty() {
        return Err(AppError::validation_msg("Name, category and department are required."));
    }
    let mut tx = db::write_tx(&state.db).await?;
    let previous: ServiceRow = sqlx::query_as("SELECT * FROM services WHERE id=?").bind(id).fetch_one(&mut *tx).await?;
    if previous.module != input.module || previous.slug != input.slug {
        return Err(AppError::conflict("Module and slug are fixed after creation."));
    }
    sqlx::query("UPDATE services SET name=?,category=?,department=? WHERE id=?")
        .bind(&input.name)
        .bind(&input.category)
        .bind(&input.department)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    catalog::reindex(&mut tx, id).await?;
    audit(&mut tx, &actor, id, "service.details_updated", json!({"name":input.name})).await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
async fn new_version(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    require_admin(&actor)?;
    let mut tx = db::write_tx(&state.db).await?;
    let module: String =
        sqlx::query_scalar("SELECT module FROM services WHERE id=?").bind(id).fetch_one(&mut *tx).await?;
    let current: Option<String> =
        sqlx::query_scalar("SELECT definition_json FROM service_versions WHERE service_id=? AND status='published'")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    let definition =
        current.map(|s| serde_json::from_str(&s)).transpose()?.unwrap_or_else(|| blank_definition(&module));
    let vid = create_version(&mut tx, &actor, id, definition, None).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":vid})))
}
pub async fn editable(tx: &mut SqliteConnection, id: i64, v: i64) -> AppResult<()> {
    let status: String = sqlx::query_scalar("SELECT status FROM service_versions WHERE id=? AND service_id=?")
        .bind(v)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if status != "draft" {
        return Err(AppError::conflict("Published and retired versions are immutable. Create a new draft."));
    }
    Ok(())
}
async fn save(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, v)): Path<(i64, i64)>,
    Json(raw): Json<Value>,
) -> AppResult<Json<Value>> {
    require_admin(&actor)?;
    let definition = raw.get("definition").unwrap_or(&raw);
    let mut tx = db::write_tx(&state.db).await?;
    editable(&mut tx, id, v).await?; // Store incomplete drafts, including unknown types, for field-level validation during publish.
    sqlx::query("UPDATE service_versions SET definition_json=? WHERE id=?")
        .bind(definition.to_string())
        .bind(v)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, &actor, id, "service.draft_saved", json!({"version_id":v})).await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
/// Validation issues of a stored definition of service `slug`, checked with the building role it will run as
/// (`documents::building::effective_role`; a non-building definition keeps its declared role so a misplaced one is
/// reported).
pub async fn issues(tx: &mut SqliteConnection, raw: &Value, module: &str, slug: &str) -> AppResult<Value> {
    match serde_json::from_value::<ServiceDefinition>(raw.clone()) {
        Ok(mut def) => {
            def.building_role =
                crate::documents::building::effective_role(module, slug, def.building_role).or(def.building_role);
            Ok(json!(validation::validate_for_module(tx, &def, module).await?))
        }
        Err(e) => Ok(json!([{"path":"definition","message":e.to_string()}])),
    }
}
pub async fn publish(tx: &mut SqliteConnection, actor: &Actor, id: i64, v: i64) -> AppResult<()> {
    require_admin(actor)?;
    editable(tx, id, v).await?;
    let (module, slug): (String, String) =
        sqlx::query_as("SELECT module,slug FROM services WHERE id=?").bind(id).fetch_one(&mut *tx).await?;
    let raw: String = sqlx::query_scalar("SELECT definition_json FROM service_versions WHERE id=?")
        .bind(v)
        .fetch_one(&mut *tx)
        .await?;
    let report = issues(tx, &serde_json::from_str(&raw)?, &module, &slug).await?;
    let issues = report.as_array().expect("issue array");
    if !issues.is_empty() {
        return Err(AppError::validation(issues.iter().map(|i| {
            (
                i["path"].as_str().unwrap_or("definition").to_owned(),
                i["message"].as_str().unwrap_or("Invalid definition").to_owned(),
            )
        })));
    }
    sqlx::query("UPDATE service_versions SET status='retired' WHERE service_id=? AND status='published'")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE service_versions SET status='published',published_by=?,published_at=? WHERE id=?")
        .bind(actor.db_id())
        .bind(time::now_str())
        .bind(v)
        .execute(&mut *tx)
        .await?;
    catalog::reindex(tx, id).await?;
    audit(tx, actor, id, "service.published", json!({"version_id":v})).await
}
async fn action(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, v, action)): Path<(i64, i64, String)>,
    Json(input): Json<Value>,
) -> AppResult<Json<Value>> {
    require_admin(&actor)?;
    let mut tx = db::write_tx(&state.db).await?;
    let (raw,module,slug):(String,String,String)=sqlx::query_as("SELECT v.definition_json,s.module,s.slug FROM service_versions v JOIN services s ON s.id=v.service_id WHERE v.id=? AND s.id=?").bind(v).bind(id).fetch_one(&mut *tx).await?;
    let definition: Value = serde_json::from_str(&raw)?;
    let result = match action.as_str() {
        "validate" => json!({"issues":issues(&mut tx,&definition,&module,&slug).await?}),
        "preview-answers" => {
            let def = ServiceDefinition::parse(&raw)?;
            match validation::validate_answers(&mut tx, &module, &def, &input["answers"]).await {
                Ok(a) => json!({"valid":true,"answers":a,"fields":{}}),
                Err(e) if e.code == crate::error::ErrorCode::Validation => json!({"valid":false,"fields":e.fields}),
                Err(e) => return Err(e),
            }
        }
        "publish" => {
            publish(&mut tx, &actor, id, v).await?;
            json!({"published":true})
        }
        "source" => {
            editable(&mut tx, id, v).await?;
            let blob = input["source_blob_id"]
                .as_i64()
                .ok_or_else(|| AppError::field("source_blob_id", "Choose an uploaded PDF."))?;
            let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM blobs WHERE id=? AND mime='application/pdf' AND scan_status IN ('clean','not_scanned') AND created_by=?)").bind(blob).bind(actor.user_id).fetch_one(&mut *tx).await?;
            if !valid {
                return Err(AppError::field("source_blob_id", "Choose your uploaded council PDF."));
            }
            sqlx::query("UPDATE service_versions SET source_blob_id=? WHERE id=?")
                .bind(blob)
                .bind(v)
                .execute(&mut *tx)
                .await?;
            audit(&mut tx, &actor, id, "service.source_attached", json!({"version_id":v,"source_blob_id":blob}))
                .await?;
            json!({"attached":true})
        }
        _ => return Err(AppError::not_found()),
    };
    tx.commit().await?;
    Ok(Json(result))
}

async fn source_file(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, v)): Path<(i64, i64)>,
    form: super::upload::Multipart,
) -> AppResult<Json<Value>> {
    require_admin(&actor)?;
    let (filename, bytes) = super::imports::uploaded_file(form).await?;
    let staged = crate::storage::stage(&state, &bytes, &filename, crate::storage::AllowList::Docs).await?;
    if staged.mime != "application/pdf" {
        return Err(AppError::field("file", "Choose a PDF form."));
    }
    let mut tx = db::write_tx(&state.db).await?;
    editable(&mut tx, id, v).await?;
    let blob = crate::storage::register(&mut tx, staged, actor.db_id()).await?;
    sqlx::query("UPDATE service_versions SET source_blob_id=? WHERE id=?")
        .bind(blob.id)
        .bind(v)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, &actor, id, "service.source_attached", json!({"version_id":v,"source_blob_id":blob.id})).await?;
    tx.commit().await?;
    Ok(Json(json!({"source_blob_id":blob.id})))
}

type VersionProjectionRow = (i64, i64, String, String, Option<i64>, Option<String>, String, Option<String>);

async fn capabilities(actor: Actor, Path(module): Path<String>) -> AppResult<Json<Value>> {
    require_admin(&actor)?;
    if !validation::MODULES.contains(&module.as_str()) {
        return Err(AppError::not_found());
    }
    Ok(Json(validation::capabilities(&module)))
}

async fn source(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, v)): Path<(i64, i64)>,
) -> AppResult<axum::response::Response> {
    use axum::response::IntoResponse;
    require_admin(&actor)?;
    let blob: Option<i64> =
        sqlx::query_scalar("SELECT source_blob_id FROM service_versions WHERE id=? AND service_id=?")
            .bind(v)
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .flatten();
    let (row, bytes) = crate::storage::read(&state, blob.ok_or_else(AppError::not_found)?).await?;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, row.mime),
            (axum::http::header::CACHE_CONTROL, "private, no-store".into()),
            (axum::http::header::CONTENT_DISPOSITION, "inline; filename=original-form.pdf".into()),
        ],
        bytes,
    )
        .into_response())
}
