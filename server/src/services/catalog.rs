use super::definition::ServiceDefinition;
use crate::{
    error::{AppError, AppResult},
    state::AppState,
    time,
    web::{Json, Path, Query},
};
use axum::{Router, extract::State, routing::get};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ServiceRow {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub category: String,
    pub module: String,
    pub department: String,
    pub is_active: bool,
    pub created_at: String,
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/public/services", get(list))
        .route("/api/public/services/{slug}", get(detail))
        .route("/api/admin/services/price-items", get(price_picker))
        .route("/api/admin/services/synonyms", get(synonyms).post(save_synonym))
}
/// FTS syntax is built from alphanumeric tokens, never interpolated from raw user input.
pub fn prefix_query(input: &str, synonyms: bool) -> String {
    let mappings = if synonyms {
        vec![
            ("party".into(), "hall".into()),
            ("birthday".into(), "hall".into()),
            ("wedding".into(), "hall".into()),
            ("venue".into(), "hall".into()),
            ("digger".into(), "equipment".into()),
            ("excavator".into(), "equipment".into()),
            ("hole".into(), "road".into()),
            ("pothole".into(), "road".into()),
            ("da".into(), "development".into()),
            ("permit".into(), "development".into()),
        ]
    } else {
        vec![]
    };
    let query = token_query(input, &mappings);
    if synonyms { query } else { query.replace(" OR ", " AND ") }
}
fn token_query(input: &str, mappings: &[(String, String)]) -> String {
    let lower = input.to_lowercase();
    let mut tokens = std::collections::BTreeSet::new();
    for word in lower.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()).take(20) {
        if ["a", "an", "the", "in", "on", "of", "for", "and", "to", "my", "i", "need", "with", "please", "is", "at"]
            .contains(&word)
        {
            continue;
        }
        tokens.insert(word.to_owned());
        for (_, replacement) in mappings.iter().filter(|(token, _)| token == word) {
            tokens
                .extend(replacement.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()).map(str::to_owned));
        }
    }
    tokens.into_iter().map(|s| format!("\"{s}\"*")).collect::<Vec<_>>().join(" OR ")
}
#[derive(Default, Deserialize)]
struct Filter {
    q: Option<String>,
    category: Option<String>,
}
async fn list(State(state): State<AppState>, Query(filter): Query<Filter>) -> AppResult<Json<Value>> {
    let mappings: Vec<(String, String)> =
        sqlx::query_as("SELECT token,replacement FROM service_synonyms").fetch_all(&state.db).await?;
    let query = token_query(filter.q.as_deref().unwrap_or(""), &mappings);
    let rows:Vec<(i64,String,String,String,String,String,String)>=sqlx::query_as("SELECT s.id,s.slug,s.name,s.category,s.module,s.department,v.definition_json FROM services s JOIN service_versions v ON v.service_id=s.id AND v.status='published' WHERE s.is_active=1 AND (? IS NULL OR s.category=?) AND (?='' OR s.id IN (SELECT service_id FROM service_search WHERE service_search MATCH ?)) ORDER BY CASE WHEN ?='' THEN 0 ELSE COALESCE((SELECT rank FROM service_search WHERE service_search MATCH ? AND service_id=s.id),0) END,s.name").bind(&filter.category).bind(&filter.category).bind(&query).bind(&query).bind(&query).bind(&query).fetch_all(&state.db).await?;
    let items:Vec<Value>=rows.into_iter().map(|(id,slug,name,category,module,department,def)|{let d:Value=serde_json::from_str(&def).unwrap_or(Value::Null);json!({"id":id,"slug":slug,"name":name,"category":category,"module":module,"department":department,"summary":d["summary"],"outcome":d["outcome"],"price_note":d["price_note"]})}).collect();
    let categories:Vec<String>=sqlx::query_scalar("SELECT DISTINCT category FROM services s WHERE is_active=1 AND EXISTS(SELECT 1 FROM service_versions v WHERE v.service_id=s.id AND v.status='published') ORDER BY category").fetch_all(&state.db).await?;
    Ok(Json(json!({"items":items,"categories":categories})))
}
pub async fn live_prices(tx: &mut SqliteConnection, def: &ServiceDefinition, today: &str) -> AppResult<Value> {
    let mut codes: Vec<String> = def.pricing.iter().map(|p| p.item.clone()).collect();
    match def.module.as_str() {
        "venue_booking" => {
            codes.extend(["HALL_MAIN_DAY", "HALL_SUPPER_DAY", "HALL_WHOLE_DAY", "HALL_BOND"].map(str::to_owned))
        }
        "equipment_hire" => codes.extend(
            ["EQUIP_EXCAVATOR_HOUR", "EQUIP_BACKHOE_HOUR", "EQUIP_TIPPER_HOUR", "EQUIP_ROLLER_HOUR", "EQUIP_EXPENSES"]
                .map(str::to_owned),
        ),
        _ => {}
    }
    let mut items = vec![];
    for code in codes {
        let row:Option<(String,String,String,i64,String)>=sqlx::query_as("SELECT i.name,i.unit,i.kind,v.amount_cents,v.effective_from FROM price_items i JOIN price_versions v ON v.price_item_id=i.id WHERE i.code=? AND v.effective_from<=? AND (v.effective_to IS NULL OR v.effective_to>?)").bind(&code).bind(today).bind(today).fetch_optional(&mut *tx).await?;
        if let Some((name, unit, kind, amount, effective_from)) = row {
            items.push(json!({"code":code,"name":name,"unit":unit,"kind":kind,"amount_cents":amount,"effective_from":effective_from}));
        }
    }
    Ok(json!(items))
}
async fn detail(State(state): State<AppState>, Path(slug): Path<String>) -> AppResult<Json<Value>> {
    let mut tx = state.db.acquire().await?;
    let service:ServiceRow=sqlx::query_as("SELECT s.* FROM services s WHERE s.slug=? AND s.is_active=1 AND EXISTS(SELECT 1 FROM service_versions v WHERE v.service_id=s.id AND v.status='published')").bind(slug).fetch_optional(&mut *tx).await?.ok_or_else(AppError::not_found)?;
    let (id, definition, note): (i64, String, Option<String>) = sqlx::query_as(
        "SELECT id,definition_json,source_note FROM service_versions WHERE service_id=? AND status='published'",
    )
    .bind(service.id)
    .fetch_one(&mut *tx)
    .await?;
    let mut def = ServiceDefinition::parse(&definition)?;
    def.module = service.module.clone();
    if def.module == "complaint" {
        let staff: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id,display_name FROM users WHERE kind='staff' AND is_active=1 ORDER BY display_name",
        )
        .fetch_all(&mut *tx)
        .await?;
        if let Some(field) = def.fields.iter_mut().find(|f| f.key == "staff_member_concerned") {
            field.options = staff
                .into_iter()
                .map(|(id, label)| super::definition::SelectOption { value: id.to_string(), label })
                .collect();
        }
    }
    let prices = live_prices(&mut tx, &def, &time::fmt_date(time::local_date(state.now()))).await?;
    Ok(Json(
        json!({"service":service,"version_id":id,"definition":def,"source_note":note,"prices":prices,"price_schedule_note":"FY2026-27 schedule (demo copy — confirm with Council)"}),
    ))
}
pub async fn reindex(tx: &mut SqliteConnection, id: i64) -> AppResult<()> {
    sqlx::query("DELETE FROM service_search WHERE service_id=?").bind(id).execute(&mut *tx).await?;
    let row:Option<(String,String)>=sqlx::query_as("SELECT s.name,v.definition_json FROM services s JOIN service_versions v ON v.service_id=s.id AND v.status='published' WHERE s.id=? AND s.is_active=1").bind(id).fetch_optional(&mut *tx).await?;
    if let Some((name, d)) = row {
        let d = ServiceDefinition::parse(&d)?;
        sqlx::query("INSERT INTO service_search(service_id,name,summary,keywords) VALUES (?,?,?,?)")
            .bind(id)
            .bind(name)
            .bind(d.summary)
            .bind(d.keywords.join(" "))
            .execute(&mut *tx)
            .await?;
    }
    Ok(())
}
async fn price_picker(State(state): State<AppState>, actor: crate::auth::Actor) -> AppResult<Json<Value>> {
    actor.require_any_role(&[crate::authz::Role::Sysadmin])?;
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT code,name,unit FROM price_items ORDER BY name").fetch_all(&state.db).await?;
    Ok(Json(
        json!({"items":rows.into_iter().map(|(code,name,unit)|json!({"code":code,"name":name,"unit":unit})).collect::<Vec<_>>()}),
    ))
}

async fn synonyms(State(state): State<AppState>, actor: crate::auth::Actor) -> AppResult<Json<Value>> {
    super::admin::require_admin(&actor)?;
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT token,replacement FROM service_synonyms ORDER BY token").fetch_all(&state.db).await?;
    Ok(Json(json!(
        rows.into_iter()
            .map(|(token, replacement)| json!({"token":token,"replacement":replacement}))
            .collect::<Vec<_>>()
    )))
}
#[derive(Deserialize)]
struct Synonym {
    token: String,
    replacement: String,
}
async fn save_synonym(
    State(state): State<AppState>,
    actor: crate::auth::Actor,
    Json(b): Json<Synonym>,
) -> AppResult<Json<Value>> {
    super::admin::require_admin(&actor)?;
    let token = b.token.trim().to_lowercase();
    let replacement = b.replacement.trim().to_lowercase();
    if token.is_empty() || token.len() > 80 || !token.chars().all(char::is_alphanumeric) || replacement.len() > 160 {
        return Err(AppError::field("token", "Use one word and a replacement of at most 160 characters."));
    }
    let mut tx = crate::db::write_tx(&state.db).await?;
    if replacement.is_empty() {
        sqlx::query("DELETE FROM service_synonyms WHERE token=?").bind(&token).execute(&mut *tx).await?;
    } else {
        sqlx::query("INSERT INTO service_synonyms VALUES(?,?) ON CONFLICT(token) DO UPDATE SET replacement=excluded.replacement").bind(&token).bind(&replacement).execute(&mut *tx).await?;
    }
    crate::audit::record(
        &mut tx,
        actor.db_id(),
        "search.synonym_saved",
        "settings",
        None,
        json!({"token":token,"replacement":replacement}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
