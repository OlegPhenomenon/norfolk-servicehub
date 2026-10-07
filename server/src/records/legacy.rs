//! Bounded RFC4180-style CSV previews, exact deduplication and atomic imports.
use super::common::{self, require_role};
use crate::{
    auth::Actor,
    authz::Role,
    cases::core::{self, NewCase, Visibility},
    db::write_tx,
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
use std::collections::{BTreeMap, HashSet};
const COLUMNS: [&str; 11] = [
    "source_system",
    "source_id",
    "service_slug",
    "applicant_name",
    "applicant_email",
    "property_ref",
    "title",
    "opened_on",
    "closed_on",
    "status",
    "notes",
];
#[derive(Clone, Serialize, Deserialize)]
pub struct PreviewRow {
    pub row: usize,
    pub source_system: String,
    pub source_id: String,
    pub title: String,
    pub fields: BTreeMap<String, String>,
    pub errors: Vec<String>,
    pub duplicate: bool,
    pub possible_duplicate: bool,
    pub case_id: Option<i64>,
}
#[derive(Serialize, Deserialize)]
pub struct Report {
    pub id: i64,
    pub status: String,
    pub rows: Vec<PreviewRow>,
    pub valid: usize,
    pub errors: usize,
    pub duplicates: usize,
    pub possible_duplicates: usize,
}
fn csv(input: &str) -> AppResult<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut after_quote = false;
    let mut chars = input.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    after_quote = true;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !after_quote => quoted = true,
            ',' => {
                row.push(std::mem::take(&mut field));
                after_quote = false;
            }
            '\n' | '\r' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                if row.iter().any(|v| !v.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
                after_quote = false;
            }
            _ if after_quote || c == '"' => return Err(AppError::field("csv", "A quoted CSV field is malformed.")),
            _ => field.push(c),
        }
        if rows.len() > 5000 {
            return Err(AppError::field("csv", "Import at most 5,000 records at once."));
        }
    }
    if quoted {
        return Err(AppError::field("csv", "A quoted CSV field is not closed."));
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}
fn mapped_status(value: &str) -> Option<&'static str> {
    match value.trim().to_lowercase().as_str() {
        "open" | "submitted" => Some("submitted"),
        "in_progress" => Some("in_progress"),
        "waiting_on_applicant" => Some("waiting_on_applicant"),
        "completed" | "closed" => Some("completed"),
        "refused" => Some("refused"),
        "withdrawn" => Some("withdrawn"),
        "cancelled" | "canceled" => Some("cancelled"),
        "closed_duplicate" => Some("closed_duplicate"),
        _ => None,
    }
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/legacy-imports", get(batches).post(preview))
        .route("/api/admin/legacy-imports/{id}", get(batch))
        .route("/api/admin/legacy-imports/{id}/import", post(import))
}
#[derive(Deserialize)]
struct Upload {
    filename: String,
    csv: String,
}
async fn preview(State(state): State<AppState>, actor: Actor, Json(body): Json<Upload>) -> AppResult<Json<Report>> {
    require_role(&actor, Role::Sysadmin)?;
    let filename = common::text(&body.filename, "filename", 200)?;
    if !filename.to_lowercase().ends_with(".csv") {
        return Err(AppError::field("filename", "Choose a CSV file."));
    }
    let parsed = csv(&body.csv)?;
    if parsed.first().is_none_or(|r| r.iter().map(String::as_str).collect::<Vec<_>>() != COLUMNS) {
        return Err(AppError::field("csv", format!("Expected columns: {}", COLUMNS.join(","))));
    }
    let staged = storage::stage(&state, body.csv.as_bytes(), &filename, AllowList::Data).await?;
    let mut tx = write_tx(&state.db).await?;
    let mut seen = HashSet::new();
    let mut profiles = HashSet::new();
    let mut report_rows = Vec::new();
    for (index, values) in parsed.into_iter().skip(1).enumerate() {
        let fields: BTreeMap<String, String> = COLUMNS
            .iter()
            .enumerate()
            .map(|(i, k)| (k.to_string(), values.get(i).cloned().unwrap_or_default().trim().into()))
            .collect();
        let mut errors = Vec::new();
        if values.len() != COLUMNS.len() {
            errors.push("Wrong number of columns.".into());
        }
        for field in ["source_system", "source_id", "service_slug", "applicant_name", "title", "opened_on", "status"] {
            if fields[field].is_empty() {
                errors.push(format!("{field} is required."));
            }
        }
        if fields.values().any(|v| v.chars().count() > 5000) {
            errors.push("A field exceeds 5,000 characters.".into());
        }
        let opened = time::parse_date(&fields["opened_on"]);
        let closed = if fields["closed_on"].is_empty() { None } else { Some(time::parse_date(&fields["closed_on"])) };
        if opened.is_err() {
            errors.push("opened_on must be YYYY-MM-DD.".into());
        }
        if let Some(c) = &closed {
            match c {
                Err(_) => errors.push("closed_on must be YYYY-MM-DD.".into()),
                Ok(c) if opened.as_ref().is_ok_and(|o| c < o) => errors.push("closed_on precedes opened_on.".into()),
                _ => {}
            }
        }
        let status = mapped_status(&fields["status"]);
        if status.is_none() {
            errors.push("Unknown status.".into());
        }
        if let Some(status) = status {
            let terminal = ["completed", "refused", "withdrawn", "cancelled", "closed_duplicate"].contains(&status);
            if terminal != closed.is_some() {
                errors.push("Closed statuses need closed_on; open statuses must leave it blank.".into());
            }
        }
        if !fields["applicant_email"].is_empty() && common::email(&fields["applicant_email"]).is_err() {
            errors.push("applicant_email is invalid.".into());
        }
        let service:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM services s JOIN service_versions v ON v.service_id=s.id WHERE s.slug=? AND v.status='published')").bind(&fields["service_slug"]).fetch_one(&mut *tx).await?;
        if !service {
            errors.push("The service has no published definition.".into());
        }
        let exact: Option<i64> = sqlx::query_scalar(
            "SELECT case_id FROM legacy_import_records WHERE source_system=? AND source_id=? AND status='imported'",
        )
        .bind(&fields["source_system"])
        .bind(&fields["source_id"])
        .fetch_optional(&mut *tx)
        .await?;
        let duplicate = exact.is_some() || !seen.insert((fields["source_system"].clone(), fields["source_id"].clone()));
        let possible:Option<i64>=sqlx::query_scalar("SELECT id FROM cases WHERE lower(applicant_name)=lower(?) AND COALESCE(property_ref,'')=? AND lower(title)=lower(?) LIMIT 1").bind(&fields["applicant_name"]).bind(&fields["property_ref"]).bind(&fields["title"]).fetch_optional(&mut *tx).await?;
        let profile =
            (fields["applicant_name"].to_lowercase(), fields["property_ref"].clone(), fields["title"].to_lowercase());
        let repeated_profile = !profiles.insert(profile);
        report_rows.push(PreviewRow {
            row: index + 2,
            source_system: fields["source_system"].clone(),
            source_id: fields["source_id"].clone(),
            title: fields["title"].clone(),
            fields,
            errors,
            duplicate,
            possible_duplicate: (possible.is_some() || repeated_profile) && !duplicate,
            case_id: exact.or(possible),
        });
    }
    let blob = storage::register(&mut tx, staged, actor.db_id()).await?;
    let mut report = Report {
        id: 0,
        status: "previewed".into(),
        valid: report_rows.iter().filter(|r| r.errors.is_empty() && !r.duplicate).count(),
        errors: report_rows.iter().filter(|r| !r.errors.is_empty()).count(),
        duplicates: report_rows.iter().filter(|r| r.duplicate).count(),
        possible_duplicates: report_rows.iter().filter(|r| r.possible_duplicate).count(),
        rows: report_rows,
    };
    report.id=sqlx::query_scalar("INSERT INTO legacy_import_batches(filename,blob_id,status,report_json,imported_by,created_at) VALUES(?,?,'previewed',?,?,?) RETURNING id").bind(filename).bind(blob.id).bind(serde_json::to_string(&report)?).bind(actor.user_id).bind(time::fmt(state.now())).fetch_one(&mut *tx).await?;
    sqlx::query("UPDATE legacy_import_batches SET report_json=? WHERE id=?")
        .bind(serde_json::to_string(&report)?)
        .bind(report.id)
        .execute(&mut *tx)
        .await?;
    common::admin_audit(
        &mut tx,
        &actor,
        "records.legacy_preview",
        "legacy_import",
        Some(report.id),
        json!({"valid":report.valid,"errors":report.errors,"duplicates":report.duplicates}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(report))
}
async fn batches(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut conn = state.db.acquire().await?;
    Ok(Json(json!(
        common::rows(
            &mut conn,
            "SELECT id,filename,status,created_at FROM legacy_import_batches ORDER BY id DESC",
            &[]
        )
        .await?
    )))
}
async fn batch(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Report>> {
    require_role(&actor, Role::Sysadmin)?;
    let raw: String = sqlx::query_scalar("SELECT report_json FROM legacy_import_batches WHERE id=?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(AppError::not_found)?;
    Ok(Json(serde_json::from_str(&raw)?))
}
#[derive(Deserialize)]
struct Import {
    skip_possible_duplicates: bool,
}
async fn import(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(body): Json<Import>,
) -> AppResult<Json<Report>> {
    require_role(&actor, Role::Sysadmin)?;
    let mut tx = write_tx(&state.db).await?;
    let (status, raw): (String, String) =
        sqlx::query_as("SELECT status,report_json FROM legacy_import_batches WHERE id=?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(AppError::not_found)?;
    let mut report: Report = serde_json::from_str(&raw)?;
    if status == "imported" {
        return Ok(Json(report));
    }
    for row in &mut report.rows {
        if !row.errors.is_empty() {
            sqlx::query("INSERT INTO legacy_import_records(batch_id,source_system,source_id,status,message) VALUES(?,?,?,'error',?)").bind(id).bind(&row.source_system).bind(&row.source_id).bind(row.errors.join(" ")).execute(&mut *tx).await?;
            continue;
        }
        let exact: Option<i64> = sqlx::query_scalar(
            "SELECT case_id FROM legacy_import_records WHERE source_system=? AND source_id=? AND status='imported'",
        )
        .bind(&row.source_system)
        .bind(&row.source_id)
        .fetch_optional(&mut *tx)
        .await?;
        let f = &row.fields;
        let possible:Option<i64>=sqlx::query_scalar("SELECT id FROM cases WHERE lower(applicant_name)=lower(?) AND COALESCE(property_ref,'')=? AND lower(title)=lower(?) LIMIT 1").bind(&f["applicant_name"]).bind(&f["property_ref"]).bind(&f["title"]).fetch_optional(&mut *tx).await?;
        if let Some(of) = exact.or(possible.filter(|_| body.skip_possible_duplicates)) {
            row.duplicate = true;
            row.case_id = Some(of);
            sqlx::query("INSERT INTO legacy_import_records(batch_id,source_system,source_id,status,duplicate_of_case_id,message) VALUES(?,?,?,'duplicate',?,'Skipped duplicate during import')").bind(id).bind(&row.source_system).bind(&row.source_id).bind(of).execute(&mut *tx).await?;
            continue;
        }
        let (sid,version,module):(i64,i64,String)=sqlx::query_as("SELECT s.id,v.id,s.module FROM services s JOIN service_versions v ON v.service_id=s.id WHERE s.slug=? AND v.status='published'").bind(&f["service_slug"]).fetch_optional(&mut *tx).await?.ok_or_else(||AppError::conflict("A service changed since preview. Upload the file again."))?;
        let status = mapped_status(&f["status"]).unwrap();
        let case = core::create_case(
            &mut tx,
            NewCase {
                service_id: sid,
                service_version_id: version,
                module,
                title: f["title"].clone(),
                status: status.into(),
                applicant_user_id: None,
                applicant_org_id: None,
                applicant_name: f["applicant_name"].clone(),
                applicant_email: (!f["applicant_email"].is_empty()).then(|| f["applicant_email"].clone()),
                applicant_phone: None,
                intake_channel: "legacy_import".into(),
                recorded_by_user_id: Some(actor.user_id),
                property_ref: (!f["property_ref"].is_empty()).then(|| f["property_ref"].clone()),
            },
        )
        .await?;
        core::assign_number(&mut tx, case.id).await?;
        let submitted = time::fmt(time::local_to_utc(time::parse_date(&f["opened_on"])?, chrono::NaiveTime::MIN));
        let closed = if f["closed_on"].is_empty() {
            None
        } else {
            Some(time::fmt(time::local_to_utc(
                time::parse_date(&f["closed_on"])?,
                chrono::NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            )))
        };
        crate::cases::core::set_import_dates(&mut tx, case.id, &submitted, closed.as_deref()).await?;
        core::append_event(
            &mut tx,
            case.id,
            actor.db_id(),
            "legacy.import",
            Visibility::Staff,
            &format!("Imported from {} record {}.", row.source_system, row.source_id),
            json!({"source_system":row.source_system,"source_id":row.source_id,"notes":f["notes"]}),
        )
        .await?;
        core::reindex_search(&mut tx, case.id).await?;
        sqlx::query("INSERT INTO legacy_import_records(batch_id,source_system,source_id,status,case_id) VALUES(?,?,?,'imported',?)").bind(id).bind(&row.source_system).bind(&row.source_id).bind(case.id).execute(&mut *tx).await?;
        row.case_id = Some(case.id);
        if closed.is_some() {
            let closed_case = core::load_case(&mut tx, case.id).await?;
            super::api::on_case_closed(&mut tx, &closed_case).await?;
        }
    }
    report.status = "imported".into();
    sqlx::query("UPDATE legacy_import_batches SET status='imported',report_json=? WHERE id=?")
        .bind(serde_json::to_string(&report)?)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    common::admin_audit(
        &mut tx,
        &actor,
        "records.legacy_import",
        "legacy_import",
        Some(id),
        json!({"skip_possible_duplicates":body.skip_possible_duplicates}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(report))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoted_csv_and_statuses() {
        assert_eq!(
            csv("a,b\r\n\"one, two\",\"line\nwith \"\"quotes\"\"\"\r\n").unwrap(),
            vec![vec!["a", "b"], vec!["one, two", "line\nwith \"quotes\""]]
        );
        assert!(csv("\"unclosed").is_err());
        assert!(csv("\"closed\"text,a").is_err());
        assert_eq!(mapped_status("canceled"), Some("cancelled"));
        assert_eq!(mapped_status("reopened"), None);
    }
}
