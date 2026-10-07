//! Building-project linkage is the only documents-owned write to cases.
use crate::{
    auth::Actor,
    authz::CaseAccess,
    cases::core::{CaseRow, Visibility},
    error::{AppError, AppResult},
    state::AppState,
    web::{Json, Path},
};
use axum::extract::State;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
fn decision_id(v: &Value) -> Option<i64> {
    v.get("decision_id")
        .and_then(Value::as_i64)
        .or_else(|| v.as_i64())
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}
pub async fn on_submit(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: &CaseRow,
    answers: &Value,
) -> AppResult<()> {
    let slug: String =
        sqlx::query_scalar("SELECT slug FROM services WHERE id=?").bind(case.service_id).fetch_one(&mut *tx).await?;
    if case.building_project_id.is_some() {
        return Ok(());
    }
    let project = match slug.as_str() {
        "development-application" => {
            let property =
                answers.get("property_ref").and_then(Value::as_str).or(case.property_ref.as_deref()).unwrap_or("");
            super::text("property_ref", property, 500)?;
            let year = crate::time::local_date(state.now()).format("%Y").to_string();
            let n: i64 =
                sqlx::query_scalar("SELECT COALESCE(MAX(id),0)+1 FROM building_projects").fetch_one(&mut *tx).await?;
            sqlx::query_scalar("INSERT INTO building_projects(reference,title,property_ref,owner_user_id,owner_org_id,created_at) VALUES(?,?,?,?,?,?) RETURNING id").bind(format!("BP-{year}-{n:06}")).bind(&case.title).bind(property).bind(case.applicant_user_id).bind(case.applicant_org_id).bind(state.now().to_rfc3339()).fetch_one(&mut *tx).await?
        }
        "modify-approval" => {
            let id = answers
                .get("original_approval")
                .and_then(decision_id)
                .ok_or_else(|| AppError::field("original_approval", "Choose an issued approval."))?;
            let (original,project):(i64,Option<i64>)=sqlx::query_as("SELECT d.case_id,c.building_project_id FROM decisions d JOIN cases c ON c.id=d.case_id WHERE d.id=? AND d.status='issued' AND d.outcome IN ('approved','approved_with_conditions') AND d.decision_type IN ('development_approval','building_approval','modification_approval')").bind(id).fetch_optional(&mut *tx).await?.ok_or_else(||AppError::field("original_approval","Choose an issued approval you can access."))?;
            if !applicant_access(tx, actor, case, original).await? {
                return Err(AppError::field(
                    "original_approval",
                    "Choose an approval visible to you as an applicant or representative.",
                ));
            }
            let project = project
                .ok_or_else(|| AppError::field("original_approval", "This approval has no building project."))?;
            sqlx::query("INSERT INTO building_original_approvals(case_id,decision_id) VALUES(?,?) ON CONFLICT(case_id) DO NOTHING").bind(case.id).bind(id).execute(&mut *tx).await?;
            link(tx, actor, case.id, original, "modification_of").await?;
            project
        }
        "building-commencement-notice" | "building-completion-notice" => {
            let v = answers
                .get("project_reference")
                .ok_or_else(|| AppError::field("project_reference", "Choose a building project."))?;
            let reference = v.as_str().unwrap_or("");
            let numeric = v.as_i64().or_else(|| reference.parse().ok());
            let project: Option<i64> = sqlx::query_scalar("SELECT id FROM building_projects WHERE id=? OR reference=?")
                .bind(numeric)
                .bind(reference)
                .fetch_optional(&mut *tx)
                .await?;
            let project =
                project.ok_or_else(|| AppError::field("project_reference", "Choose a project you can access."))?;
            let originals: Vec<i64> =
                sqlx::query_scalar("SELECT id FROM cases WHERE building_project_id=? AND id<>? ORDER BY id")
                    .bind(project)
                    .bind(case.id)
                    .fetch_all(&mut *tx)
                    .await?;
            let mut original = None;
            for id in originals {
                if applicant_access(tx, actor, case, id).await? {
                    original = Some(id);
                    break;
                }
            }
            let original = original.ok_or_else(|| {
                AppError::field(
                    "project_reference",
                    "Choose a project visible to you as an applicant or representative.",
                )
            })?;
            link(tx, actor, case.id, original, "follow_up_of").await?;
            project
        }
        _ => return Ok(()),
    };
    sqlx::query("UPDATE cases SET building_project_id=? WHERE id=?")
        .bind(project)
        .bind(case.id)
        .execute(&mut *tx)
        .await?;
    super::changed(
        tx,
        actor.db_id(),
        case.id,
        "documents.project_link",
        Visibility::Applicant,
        "Request linked to its building project.",
    )
    .await?;
    Ok(())
}
async fn applicant_access(tx: &mut SqliteConnection, actor: &Actor, case: &CaseRow, target: i64) -> AppResult<bool> {
    if crate::authz::case_access(tx, actor, case.id).await? == CaseAccess::Applicant {
        return Ok(crate::authz::case_access(tx, actor, target).await? == CaseAccess::Applicant);
    }
    if !crate::authz::case_access(tx, actor, case.id).await?.can_manage() {
        return Ok(false);
    }
    let candidates: Vec<i64> = if let Some(org) = case.applicant_org_id {
        sqlx::query_scalar("SELECT user_id FROM memberships WHERE organisation_id=? AND status='active'")
            .bind(org)
            .fetch_all(&mut *tx)
            .await?
    } else {
        case.applicant_user_id.into_iter().collect()
    };
    for id in candidates {
        let applicant = Actor::load(tx, id, true).await?;
        if crate::authz::case_access(tx, &applicant, target).await? == CaseAccess::Applicant {
            return Ok(true);
        }
    }
    Ok(false)
}
async fn link(tx: &mut SqliteConnection, actor: &Actor, from: i64, to: i64, kind: &str) -> AppResult<()> {
    sqlx::query("INSERT INTO case_links(from_case_id,to_case_id,kind,created_by,created_at) VALUES(?,?,?,?,?) ON CONFLICT DO NOTHING").bind(from).bind(to).bind(kind).bind(actor.db_id()).bind(crate::time::now_str()).execute(tx).await?;
    Ok(())
}
pub async fn approvals(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Vec<Value>>> {
    let mut c = state.db.acquire().await?;
    let rows=sqlx::query("SELECT d.id,d.case_id,d.decision_type,d.issued_at,c.number,c.building_project_id,c.property_ref FROM decisions d JOIN cases c ON c.id=d.case_id WHERE d.status='issued' AND d.outcome IN ('approved','approved_with_conditions') AND d.decision_type IN ('development_approval','building_approval','modification_approval') ORDER BY d.id DESC").fetch_all(&mut *c).await?;
    let mut out = vec![];
    for r in rows {
        let id: i64 = r.get("case_id");
        if crate::authz::case_access(&mut c, &actor, id).await? == CaseAccess::Applicant {
            out.push(json!({"id":r.get::<i64,_>("id"),"decision_type":r.get::<String,_>("decision_type"),"issued_at":r.get::<String,_>("issued_at"),"case_number":r.get::<Option<String>,_>("number"),"project_id":r.get::<Option<i64>,_>("building_project_id"),"property_ref":r.get::<Option<String>,_>("property_ref")}));
        }
    }
    Ok(Json(out))
}
pub async fn detail(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut c = state.db.acquire().await?;
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM cases WHERE building_project_id=? ORDER BY id")
        .bind(id)
        .fetch_all(&mut *c)
        .await?;
    let mut allowed = Vec::new();
    for cid in ids {
        let a = crate::authz::case_access(&mut c, &actor, cid).await?;
        if matches!(a, CaseAccess::Applicant | CaseAccess::Staff { .. }) {
            allowed.push((cid, a));
        }
    }
    if allowed.is_empty() {
        return Err(AppError::not_found());
    }
    let r = sqlx::query("SELECT reference,title,property_ref,created_at FROM building_projects WHERE id=?")
        .bind(id)
        .fetch_one(&mut *c)
        .await?;
    let mut cases = Vec::new();
    let mut decisions = Vec::new();
    for (cid, a) in allowed {
        let case = crate::cases::core::load_case(&mut c, cid).await?;
        let links: Vec<(i64, String)> = sqlx::query_as("SELECT to_case_id,kind FROM case_links WHERE from_case_id=?")
            .bind(cid)
            .fetch_all(&mut *c)
            .await?;
        let mut visible_links = vec![];
        for (target, kind) in links {
            if matches!(
                crate::authz::case_access(&mut c, &actor, target).await?,
                CaseAccess::Applicant | CaseAccess::Staff { .. }
            ) {
                visible_links.push(json!({"case_id":target,"kind":kind}));
            }
        }
        cases.push(json!({"id":case.id,"number":case.number,"title":case.title,"status":case.status,"created_at":case.created_at,"links":visible_links}));
        let mut ds: Vec<super::decisions::Decision> =
            sqlx::query_as("SELECT * FROM decisions WHERE case_id=? AND (? OR status='issued') ORDER BY id")
                .bind(cid)
                .bind(a.is_staff())
                .fetch_all(&mut *c)
                .await?;
        for d in &mut ds {
            d.evidence = super::decisions::evidence(&mut c, d.id).await?;
        }
        decisions.extend(ds);
    }
    decisions.sort_by_key(|d| d.id);
    Ok(Json(
        json!({"id":id,"reference":r.get::<String,_>("reference"),"title":r.get::<String,_>("title"),"property_ref":r.get::<String,_>("property_ref"),"created_at":r.get::<String,_>("created_at"),"cases":cases,"decisions":decisions}),
    ))
}
