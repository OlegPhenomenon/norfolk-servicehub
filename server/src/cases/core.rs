//! Case primitives (owner: platform). Every function takes the caller's connection/transaction so
//! composite commands stay atomic.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqliteConnection;

use crate::error::{AppError, AppResult};
use crate::time;

/// A full row of the `cases` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct CaseRow {
    pub id: i64,
    pub number: Option<String>,
    pub service_id: i64,
    pub service_version_id: i64,
    /// Frozen copy of `services.module` at creation: drives `hooks` dispatch.
    pub module: String,
    pub title: String,
    pub status: String,
    pub current_step: Option<String>,
    pub applicant_user_id: Option<i64>,
    pub applicant_org_id: Option<i64>,
    pub applicant_name: String,
    pub applicant_email: Option<String>,
    pub applicant_phone: Option<String>,
    pub intake_channel: String,
    pub recorded_by_user_id: Option<i64>,
    pub confidential: i64,
    pub public_map: i64,
    pub property_ref: Option<String>,
    pub location_text: Option<String>,
    pub location_lat: Option<f64>,
    pub location_lng: Option<f64>,
    pub building_project_id: Option<i64>,
    pub reopened_count: i64,
    pub legal_hold: i64,
    pub retention_until: Option<String>,
    pub revision: i64,
    pub created_at: String,
    pub submitted_at: Option<String>,
    pub closed_at: Option<String>,
    pub updated_at: String,
}

impl CaseRow {
    pub fn is_confidential(&self) -> bool {
        self.confidential == 1
    }
}

/// Input for [`create_case`]. `module` must be the service's module at creation time.
#[derive(Debug, Clone)]
pub struct NewCase {
    pub service_id: i64,
    pub service_version_id: i64,
    pub module: String,
    pub title: String,
    /// Usually `"draft"`; assisted intake may create `"submitted"` directly.
    pub status: String,
    pub applicant_user_id: Option<i64>,
    pub applicant_org_id: Option<i64>,
    pub applicant_name: String,
    pub applicant_email: Option<String>,
    pub applicant_phone: Option<String>,
    /// `online`, `phone`, `walk_in`, `email`, `post`, `legacy_import`.
    pub intake_channel: String,
    pub recorded_by_user_id: Option<i64>,
    pub property_ref: Option<String>,
}

/// `case_events.visibility` (also used for `documents.visibility`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// Shown to the applicant and to staff.
    Applicant,
    /// Staff with case access only.
    Staff,
}

impl Visibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Visibility::Applicant => "applicant",
            Visibility::Staff => "staff",
        }
    }
}

/// Inserts a case. Complaint cases (`module = "complaint"`) are created `confidential = 1`.
pub async fn create_case(conn: &mut SqliteConnection, new: NewCase) -> AppResult<CaseRow> {
    let now = time::now_str();
    let confidential = i64::from(new.module == "complaint");
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO cases (service_id, service_version_id, module, title, status, applicant_user_id, applicant_org_id, \
         applicant_name, applicant_email, applicant_phone, intake_channel, recorded_by_user_id, confidential, property_ref, \
         created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(new.service_id)
    .bind(new.service_version_id)
    .bind(&new.module)
    .bind(&new.title)
    .bind(&new.status)
    .bind(new.applicant_user_id)
    .bind(new.applicant_org_id)
    .bind(&new.applicant_name)
    .bind(&new.applicant_email)
    .bind(&new.applicant_phone)
    .bind(&new.intake_channel)
    .bind(new.recorded_by_user_id)
    .bind(confidential)
    .bind(&new.property_ref)
    .bind(&now)
    .bind(&now)
    .fetch_one(&mut *conn)
    .await?;
    load_case(conn, id).await
}

/// Loads a case row; `not_found` when missing. Does **not** check access — use `authz::require_case` in handlers.
pub async fn load_case(conn: &mut SqliteConnection, case_id: i64) -> AppResult<CaseRow> {
    sqlx::query_as::<_, CaseRow>("SELECT * FROM cases WHERE id = ?")
        .bind(case_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(AppError::not_found)
}

/// Appends a timeline event with a plain-English `summary`. Returns the event id.
pub async fn append_event(
    conn: &mut SqliteConnection,
    case_id: i64,
    actor: Option<i64>,
    kind: &str,
    visibility: Visibility,
    summary: &str,
    data: Value,
) -> AppResult<i64> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO case_events (case_id, at, actor_user_id, kind, visibility, summary, data_json) \
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(case_id)
    .bind(time::now_str())
    .bind(actor)
    .bind(kind)
    .bind(visibility.as_str())
    .bind(summary)
    .bind(data.to_string())
    .fetch_one(&mut *conn)
    .await?;
    Ok(id)
}

/// Optimistic concurrency: increments `revision` (and `updated_at`). With `expected = Some(r)` the
/// update only happens when the current revision is `r`, else `stale_revision`. Returns the new revision.
pub async fn bump_revision(conn: &mut SqliteConnection, case_id: i64, expected: Option<i64>) -> AppResult<i64> {
    let new_rev: Option<i64> = sqlx::query_scalar(
        "UPDATE cases SET revision = revision + 1, updated_at = ? WHERE id = ? AND (? IS NULL OR revision = ?) RETURNING revision",
    )
    .bind(time::now_str())
    .bind(case_id)
    .bind(expected)
    .bind(expected)
    .fetch_optional(&mut *conn)
    .await?;
    match new_rev {
        Some(r) => Ok(r),
        None => {
            let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM cases WHERE id = ?)")
                .bind(case_id)
                .fetch_one(&mut *conn)
                .await?;
            Err(if exists { AppError::stale_revision() } else { AppError::not_found() })
        }
    }
}

/// Allocates `NSH-<year>-<6-digit seq>` (Norfolk local year) inside the caller's write transaction.
/// Idempotent: returns the existing number if the case already has one.
pub async fn assign_number(conn: &mut SqliteConnection, case_id: i64) -> AppResult<String> {
    let existing: Option<Option<String>> =
        sqlx::query_scalar("SELECT number FROM cases WHERE id = ?").bind(case_id).fetch_optional(&mut *conn).await?;
    match existing {
        None => return Err(AppError::not_found()),
        Some(Some(n)) => return Ok(n),
        Some(None) => {}
    }
    let year = time::to_local(chrono::Utc::now()).format("%Y").to_string();
    let prefix = format!("NSH-{year}-");
    let last: Option<String> =
        sqlx::query_scalar("SELECT number FROM cases WHERE number LIKE ? ORDER BY number DESC LIMIT 1")
            .bind(format!("{prefix}%"))
            .fetch_optional(&mut *conn)
            .await?;
    let next = last.and_then(|n| n.strip_prefix(&prefix).and_then(|s| s.parse::<u32>().ok())).unwrap_or(0) + 1;
    let number = format!("{prefix}{next:06}");
    sqlx::query("UPDATE cases SET number = ?, updated_at = ? WHERE id = ?")
        .bind(&number)
        .bind(time::now_str())
        .bind(case_id)
        .execute(&mut *conn)
        .await?;
    Ok(number)
}

/// Collects string leaves of a JSON value (answers) for the search body.
fn collect_text(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Number(n) => out.push(n.to_string()),
        Value::Array(a) => a.iter().for_each(|x| collect_text(x, out)),
        Value::Object(o) => o.values().for_each(|x| collect_text(x, out)),
        _ => {}
    }
}

/// Writes/replaces the case's row in the `case_search` FTS table (number, title, applicant, property,
/// plus answer text and location description).
pub async fn reindex_search(conn: &mut SqliteConnection, case_id: i64) -> AppResult<()> {
    let case = load_case(conn, case_id).await?;
    // Submitted answers win over the draft.
    let answers: Option<String> = sqlx::query_scalar(
        "SELECT COALESCE((SELECT answers_json FROM submissions WHERE case_id = ?), \
                         (SELECT answers_json FROM case_drafts WHERE case_id = ?))",
    )
    .bind(case_id)
    .bind(case_id)
    .fetch_one(&mut *conn)
    .await?;
    let mut parts = Vec::new();
    if let Some(a) = answers.and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
        collect_text(&a, &mut parts);
    }
    if let Some(l) = &case.location_text {
        parts.push(l.clone());
    }
    sqlx::query("DELETE FROM case_search WHERE case_id = ?").bind(case_id).execute(&mut *conn).await?;
    sqlx::query(
        "INSERT INTO case_search (case_id, number, title, applicant_name, property_ref, body) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(case_id)
    .bind(case.number.unwrap_or_default())
    .bind(&case.title)
    .bind(&case.applicant_name)
    .bind(case.property_ref.unwrap_or_default())
    .bind(parts.join(" "))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_number_event_revision_search() {
        let (state, _dir) = crate::state::test_support::test_state().await;
        let mut tx = crate::db::write_tx(&state.db).await.unwrap();
        let now = time::now_str();
        for (sid, module) in [(1, "generic"), (2, "complaint")] {
            sqlx::query("INSERT INTO services (id, slug, name, category, module, department, created_at) VALUES (?, ?, 'S', 'C', ?, 'D', ?)")
                .bind(sid).bind(format!("s{sid}")).bind(module).bind(&now).execute(&mut *tx).await.unwrap();
            sqlx::query("INSERT INTO service_versions (id, service_id, version, status, definition_json, created_at) VALUES (?, ?, 1, 'published', '{}', ?)")
                .bind(sid).bind(sid).bind(&now).execute(&mut *tx).await.unwrap();
        }
        let new = |sid: i64, module: &str| NewCase {
            service_id: sid,
            service_version_id: sid,
            module: module.into(),
            title: "Hall hire for Bounty Day".into(),
            status: "submitted".into(),
            applicant_user_id: None,
            applicant_org_id: None,
            applicant_name: "Alexey Turner".into(),
            applicant_email: None,
            applicant_phone: None,
            intake_channel: "phone".into(),
            recorded_by_user_id: None,
            property_ref: Some("Portion 44h".into()),
        };
        let c1 = create_case(&mut tx, new(1, "generic")).await.unwrap();
        assert_eq!(c1.confidential, 0);
        let c2 = create_case(&mut tx, new(2, "complaint")).await.unwrap();
        assert_eq!(c2.confidential, 1);

        let n1 = assign_number(&mut tx, c1.id).await.unwrap();
        let n2 = assign_number(&mut tx, c2.id).await.unwrap();
        assert!(n1.starts_with("NSH-") && n1.ends_with("-000001"), "{n1}");
        assert!(n2.ends_with("-000002"), "{n2}");
        assert_eq!(assign_number(&mut tx, c1.id).await.unwrap(), n1);

        append_event(
            &mut tx,
            c1.id,
            None,
            "submitted",
            Visibility::Applicant,
            "Request submitted.",
            serde_json::json!({}),
        )
        .await
        .unwrap();
        assert_eq!(bump_revision(&mut tx, c1.id, Some(1)).await.unwrap(), 2);
        assert_eq!(
            bump_revision(&mut tx, c1.id, Some(1)).await.unwrap_err().code,
            crate::error::ErrorCode::StaleRevision
        );
        assert_eq!(bump_revision(&mut tx, 999, None).await.unwrap_err().code, crate::error::ErrorCode::NotFound);

        reindex_search(&mut tx, c1.id).await.unwrap();
        reindex_search(&mut tx, c1.id).await.unwrap();
        let hits: Vec<i64> = sqlx::query_scalar("SELECT case_id FROM case_search WHERE case_search MATCH 'bounty'")
            .fetch_all(&mut *tx)
            .await
            .unwrap();
        assert_eq!(hits, vec![c1.id]);
        tx.commit().await.unwrap();
    }
}
