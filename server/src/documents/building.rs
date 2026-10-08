//! Building-project linkage is the only documents-owned write to cases.
use crate::services::definition::{BuildingRole, FieldType, ServiceDefinition};
use crate::{
    auth::{Actor, StaffActor},
    authz::CaseAccess,
    cases::core::{CaseRow, Visibility},
    error::{AppError, AppResult},
    state::AppState,
    web::{Json, Path, Query},
};
use axum::extract::State;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
/// Approved, issued DA/BA/modification decision that no issued approved modification has superseded (alias `d`).
pub(crate) const CURRENT_APPROVAL: &str = "d.status='issued' AND d.outcome IN ('approved','approved_with_conditions') AND d.decision_type IN ('development_approval','building_approval','modification_approval') AND NOT EXISTS(SELECT 1 FROM decisions s WHERE s.supersedes_decision_id=d.id AND s.status='issued' AND s.outcome IN ('approved','approved_with_conditions'))";
pub const APPROVAL_TYPES: [&str; 2] = ["development_approval", "building_approval"];
/// `original_approval` answers: `{"decision_ids":[..]}` (multiple) or the legacy `{"decision_id":n}`.
pub fn decision_ids(v: &Value) -> Option<Vec<i64>> {
    if let Some(list) = v.get("decision_ids").and_then(Value::as_array) {
        return list.iter().map(Value::as_i64).collect();
    }
    v.get("decision_id")
        .and_then(Value::as_i64)
        .or_else(|| v.as_i64())
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .map(|id| vec![id])
}
/// Role of a case that runs an approval route (a project's DA/BA or a modification of issued approvals):
/// fee assessment, approval scope and exhibition rules apply. Follow-up notices and other building
/// services have none.
pub async fn approval_role(tx: &mut SqliteConnection, case: &CaseRow) -> AppResult<Option<BuildingRole>> {
    Ok(role_of(tx, case).await?.filter(|r| matches!(r, BuildingRole::Project | BuildingRole::Modification)))
}
/// The case's building role, read from its frozen definition. A definition without `building_role` that
/// belongs to one of the building services seeded before the field existed (seeded or staff-edited versions
/// published before the upgrade) keeps the role those services always had, by service slug.
pub async fn role_of(tx: &mut SqliteConnection, case: &CaseRow) -> AppResult<Option<BuildingRole>> {
    if case.module != "building" {
        return Ok(None);
    }
    let def = crate::services::definition::load_for_case(tx, case).await?;
    role_in(tx, case, &def).await
}
async fn role_in(
    tx: &mut SqliteConnection,
    case: &CaseRow,
    def: &ServiceDefinition,
) -> AppResult<Option<BuildingRole>> {
    if case.module != "building" || def.building_role.is_some() {
        return Ok(def.building_role.filter(|_| case.module == "building"));
    }
    let slug: String =
        sqlx::query_scalar("SELECT slug FROM services WHERE id=?").bind(case.service_id).fetch_one(&mut *tx).await?;
    Ok(effective_role(&case.module, &slug, None))
}
/// The building role a definition of service `slug` runs with: its declared `building_role`, else — for the
/// building services that existed before the field, whose slugs were the role — the role of the slug. Runtime
/// (`role_of`) and publish validation both use this, so a definition is validated with the role it will run as.
pub fn effective_role(module: &str, slug: &str, declared: Option<BuildingRole>) -> Option<BuildingRole> {
    if module != "building" {
        return None;
    }
    declared.or(match slug {
        "development-application" => Some(BuildingRole::Project),
        "modify-approval" => Some(BuildingRole::Modification),
        "building-commencement-notice" | "building-completion-notice" => Some(BuildingRole::FollowUp),
        _ => None,
    })
}
/// Decision types a building role may receive: a project its development and/or building approval (its
/// confirmed scope decides which), a modification only a modification approval, and a follow-up notice only
/// the written permission to continue (`service_response`). Without a role nothing links the request to the
/// approvals it would modify, so a modification approval is never possible.
pub fn role_permits(role: Option<BuildingRole>, decision_type: &str) -> bool {
    match role {
        Some(BuildingRole::Modification) => decision_type == "modification_approval",
        Some(BuildingRole::Project) => APPROVAL_TYPES.contains(&decision_type),
        Some(BuildingRole::FollowUp) => decision_type == "service_response",
        None => decision_type != "modification_approval",
    }
}
pub async fn on_submit(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: &CaseRow,
    answers: &Value,
) -> AppResult<()> {
    if case.building_project_id.is_some() {
        return Ok(());
    }
    let def = crate::services::definition::load_for_case(tx, case).await?;
    let Some(role) = role_in(tx, case, &def).await? else {
        return Ok(());
    };
    let project = match role {
        BuildingRole::Project => {
            let property =
                answers.get("property_ref").and_then(Value::as_str).or(case.property_ref.as_deref()).unwrap_or("");
            super::text("property_ref", property, 500)?;
            let year = crate::time::local_date(state.now()).format("%Y").to_string();
            let n: i64 =
                sqlx::query_scalar("SELECT COALESCE(MAX(id),0)+1 FROM building_projects").fetch_one(&mut *tx).await?;
            let project: i64 = sqlx::query_scalar("INSERT INTO building_projects(reference,title,property_ref,owner_user_id,owner_org_id,created_at) VALUES(?,?,?,?,?,?) RETURNING id").bind(format!("BP-{year}-{n:06}")).bind(&case.title).bind(property).bind(case.applicant_user_id).bind(case.applicant_org_id).bind(state.now().to_rfc3339()).fetch_one(&mut *tx).await?;
            if let Some(sought) = answers.get("approvals_sought").and_then(Value::as_array) {
                let approvals: Vec<&str> =
                    APPROVAL_TYPES.into_iter().filter(|t| sought.iter().any(|v| v.as_str() == Some(t))).collect();
                if approvals.is_empty() {
                    return Err(AppError::field("approvals_sought", "Choose the approvals you are applying for."));
                }
                record_scope(
                    tx,
                    case.id,
                    &json!({"approvals":approvals}),
                    None,
                    "Approvals sought in the application.",
                )
                .await?;
            }
            project
        }
        BuildingRole::Modification => {
            let ids = answers
                .get("original_approval")
                .and_then(decision_ids)
                .filter(|ids| !ids.is_empty())
                .ok_or_else(|| AppError::field("original_approval", "Choose an issued approval."))?;
            let mut project = None;
            let mut originals = vec![];
            for id in &ids {
                let (original,p):(i64,Option<i64>)=sqlx::query_as(&format!("SELECT d.case_id,c.building_project_id FROM decisions d JOIN cases c ON c.id=d.case_id WHERE d.id=? AND {CURRENT_APPROVAL}")).bind(id).fetch_optional(&mut *tx).await?.ok_or_else(||AppError::field("original_approval","Choose a current issued approval you can access."))?;
                if !may_link(tx, actor, case, original).await? {
                    return Err(AppError::field(
                        "original_approval",
                        if assisted_intake(actor, case) {
                            "Choose the applicant's issued approval from a request you can access."
                        } else {
                            "Choose an approval visible to you as an applicant or representative."
                        },
                    ));
                }
                let p =
                    p.ok_or_else(|| AppError::field("original_approval", "This approval has no building project."))?;
                if project.is_some_and(|x| x != p) {
                    return Err(AppError::field("original_approval", "Choose approvals of the same building project."));
                }
                project = Some(p);
                sqlx::query(
                    "INSERT INTO building_original_approvals(case_id,decision_id) VALUES(?,?) ON CONFLICT DO NOTHING",
                )
                .bind(case.id)
                .bind(id)
                .execute(&mut *tx)
                .await?;
                if !originals.contains(&original) {
                    originals.push(original);
                    link(tx, actor, case.id, original, "modification_of").await?;
                }
            }
            record_scope(tx, case.id, &json!({"originals":ids}), None, "Approvals named in the application.").await?;
            project.expect("at least one original approval")
        }
        BuildingRole::FollowUp => follow_up(tx, actor, case, &def, answers).await?,
    };
    sqlx::query("UPDATE cases SET building_project_id=? WHERE id=?")
        .bind(project)
        .bind(case.id)
        .execute(&mut *tx)
        .await?;
    let reference: String = sqlx::query_scalar("SELECT reference FROM building_projects WHERE id=?")
        .bind(project)
        .fetch_one(&mut *tx)
        .await?;
    let summary = if assisted_intake(actor, case) && !matches!(role, BuildingRole::Project) {
        format!(
            "Request linked to building project {reference}, chosen by Council on the applicant's behalf when recording the request."
        )
    } else {
        "Request linked to its building project.".to_string()
    };
    super::changed(tx, actor.db_id(), case.id, "documents.project_link", Visibility::Applicant, &summary).await?;
    Ok(())
}
/// Staff recording a phone, walk-in, email or post request (`/staff/intake`) choose the applicant's project or
/// approval by reference on the applicant's behalf; such a case has no applicant account to check against.
fn assisted_intake(actor: &Actor, case: &CaseRow) -> bool {
    actor.is_staff()
        && case.intake_channel != "online"
        && case.recorded_by_user_id.is_some()
        && case.recorded_by_user_id == actor.db_id()
        && actor.roles_for_service(case.service_id).contains(&crate::authz::Role::Intake)
}
/// May `case` be linked to the earlier request `target`: the applicant's own request, or — for assisted intake —
/// one the recording staff member can access.
async fn may_link(tx: &mut SqliteConnection, actor: &Actor, case: &CaseRow, target: i64) -> AppResult<bool> {
    if assisted_intake(actor, case) {
        return Ok(crate::authz::case_access(tx, actor, target).await?.is_staff());
    }
    applicant_access(tx, actor, case, target).await
}
/// Links a follow-up (commencement, stage or completion notice) to the project named by its `project_ref`
/// answer: a project ID or reference typed by the applicant or chosen from `/api/my/building-projects`, or — for
/// assisted intake — chosen by staff from `/api/staff/building-projects`.
async fn follow_up(
    tx: &mut SqliteConnection,
    actor: &Actor,
    case: &CaseRow,
    def: &ServiceDefinition,
    answers: &Value,
) -> AppResult<i64> {
    let key = def
        .fields
        .iter()
        .find(|f| f.field_type == FieldType::ProjectRef)
        .map_or("project_reference", |f| f.key.as_str());
    let v = answers.get(key).ok_or_else(|| AppError::field(key, "Choose a building project."))?;
    let reference = v.as_str().map(str::trim).unwrap_or("");
    let numeric = v.as_i64().or_else(|| reference.parse().ok());
    let project: Option<i64> = sqlx::query_scalar("SELECT id FROM building_projects WHERE id=? OR reference=?")
        .bind(numeric)
        .bind(reference)
        .fetch_optional(&mut *tx)
        .await?;
    let project = project.ok_or_else(|| AppError::field(key, "Choose a project you can access."))?;
    let originals: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM cases WHERE building_project_id=? AND id<>? ORDER BY id")
            .bind(project)
            .bind(case.id)
            .fetch_all(&mut *tx)
            .await?;
    let mut original = None;
    for id in originals {
        if may_link(tx, actor, case, id).await? {
            original = Some(id);
            break;
        }
    }
    let original = original.ok_or_else(|| {
        AppError::field(
            key,
            if assisted_intake(actor, case) {
                "Choose the applicant's building project from a request you can access."
            } else {
                "Choose a project visible to you as an applicant or representative."
            },
        )
    })?;
    link(tx, actor, case.id, original, "follow_up_of").await?;
    Ok(project)
}
/// Building projects the actor can lodge follow-up notices against (applicant or representative access to at
/// least one request), with their issued approvals, for the follow-up project picker.
pub async fn my_projects(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Vec<Value>>> {
    let mut c = state.db.acquire().await?;
    let projects: Vec<(i64, String, String, String)> =
        sqlx::query_as("SELECT id,reference,title,property_ref FROM building_projects ORDER BY id DESC")
            .fetch_all(&mut *c)
            .await?;
    Ok(Json(project_list(&mut c, &actor, projects, false).await?))
}
#[derive(serde::Deserialize)]
pub struct Lookup {
    q: Option<String>,
}
/// `LIKE` pattern for a staff lookup; at least two characters, so staff search rather than browse every project.
fn lookup_pattern(q: Option<&str>) -> Option<String> {
    let q = q?.trim();
    (q.chars().count() >= 2).then(|| format!("%{}%", q.replace(['%', '_'], "")))
}
/// Staff-recorded (assisted) intake: find the applicant's building project by project reference, request number,
/// property or applicant name, among requests the staff member can access.
pub async fn staff_projects(
    State(state): State<AppState>,
    StaffActor(actor): StaffActor,
    Query(lookup): Query<Lookup>,
) -> AppResult<Json<Vec<Value>>> {
    actor.require_any_role(&[crate::authz::Role::Intake])?;
    let Some(pattern) = lookup_pattern(lookup.q.as_deref()) else {
        return Ok(Json(vec![]));
    };
    let mut c = state.db.acquire().await?;
    let projects: Vec<(i64, String, String, String)> = sqlx::query_as("SELECT p.id,p.reference,p.title,p.property_ref FROM building_projects p WHERE p.reference LIKE ?1 OR p.title LIKE ?1 OR p.property_ref LIKE ?1 OR EXISTS(SELECT 1 FROM cases c WHERE c.building_project_id=p.id AND (c.number LIKE ?1 OR c.applicant_name LIKE ?1)) ORDER BY p.id DESC LIMIT 50")
        .bind(pattern)
        .fetch_all(&mut *c)
        .await?;
    Ok(Json(project_list(&mut c, &actor, projects, true).await?))
}
/// Projects with their issued approvals, keeping those with a request the actor can see from their side
/// (applicant/representative, or staff for assisted intake).
async fn project_list(
    c: &mut SqliteConnection,
    actor: &Actor,
    projects: Vec<(i64, String, String, String)>,
    staff: bool,
) -> AppResult<Vec<Value>> {
    let mut out = vec![];
    for (id, reference, title, property_ref) in projects {
        let cases: Vec<i64> = sqlx::query_scalar("SELECT id FROM cases WHERE building_project_id=? ORDER BY id")
            .bind(id)
            .fetch_all(&mut *c)
            .await?;
        let mut visible = false;
        for case in cases {
            let access = crate::authz::case_access(&mut *c, actor, case).await?;
            let seen = if staff { access.is_staff() } else { access == CaseAccess::Applicant };
            if seen {
                visible = true;
                break;
            }
        }
        if !visible {
            continue;
        }
        let approvals: Vec<(i64, String, Option<String>, Option<String>)> = sqlx::query_as("SELECT d.id,d.decision_type,d.issued_at,c.number FROM decisions d JOIN cases c ON c.id=d.case_id WHERE c.building_project_id=? AND d.status='issued' AND d.outcome IN ('approved','approved_with_conditions') AND d.decision_type IN ('development_approval','building_approval','modification_approval') ORDER BY d.id")
            .bind(id).fetch_all(&mut *c).await?;
        out.push(json!({"id":id,"reference":reference,"title":title,"property_ref":property_ref,"approvals":approvals.into_iter().map(|(id,t,at,number)|json!({"id":id,"decision_type":t,"issued_at":at,"case_number":number})).collect::<Vec<_>>()}));
    }
    Ok(out)
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
    let rows=sqlx::query(&format!("SELECT d.id,d.case_id,d.decision_type,d.issued_at,c.number,c.building_project_id,c.property_ref FROM decisions d JOIN cases c ON c.id=d.case_id WHERE {CURRENT_APPROVAL} ORDER BY d.id DESC")).fetch_all(&mut *c).await?;
    Ok(Json(approval_list(&mut c, &actor, rows, false).await?))
}
/// Staff-recorded (assisted) intake: find the applicant's current issued approvals by request number, project
/// reference, property or applicant name, among requests the staff member can access.
pub async fn staff_approvals(
    State(state): State<AppState>,
    StaffActor(actor): StaffActor,
    Query(lookup): Query<Lookup>,
) -> AppResult<Json<Vec<Value>>> {
    actor.require_any_role(&[crate::authz::Role::Intake])?;
    let Some(pattern) = lookup_pattern(lookup.q.as_deref()) else {
        return Ok(Json(vec![]));
    };
    let mut c = state.db.acquire().await?;
    let rows=sqlx::query(&format!("SELECT d.id,d.case_id,d.decision_type,d.issued_at,c.number,c.building_project_id,c.property_ref FROM decisions d JOIN cases c ON c.id=d.case_id LEFT JOIN building_projects p ON p.id=c.building_project_id WHERE {CURRENT_APPROVAL} AND (c.number LIKE ?1 OR c.property_ref LIKE ?1 OR c.applicant_name LIKE ?1 OR p.reference LIKE ?1) ORDER BY d.id DESC LIMIT 50")).bind(pattern).fetch_all(&mut *c).await?;
    Ok(Json(approval_list(&mut c, &actor, rows, true).await?))
}
async fn approval_list(
    c: &mut SqliteConnection,
    actor: &Actor,
    rows: Vec<sqlx::sqlite::SqliteRow>,
    staff: bool,
) -> AppResult<Vec<Value>> {
    let mut out = vec![];
    for r in rows {
        let access = crate::authz::case_access(&mut *c, actor, r.get("case_id")).await?;
        let seen = if staff { access.is_staff() } else { access == CaseAccess::Applicant };
        if seen {
            let approval_type = root_type(&mut *c, r.get("id")).await?;
            out.push(json!({"id":r.get::<i64,_>("id"),"decision_type":r.get::<String,_>("decision_type"),"approval_type":approval_type,"issued_at":r.get::<String,_>("issued_at"),"case_number":r.get::<Option<String>,_>("number"),"project_id":r.get::<Option<i64>,_>("building_project_id"),"property_ref":r.get::<Option<String>,_>("property_ref")}));
        }
    }
    Ok(out)
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
    let mut history = Vec::new();
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
        let documents = super::uploads::project(&mut c, cid, a).await?;
        let events: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT at,kind,summary,data_json FROM case_events WHERE case_id=? AND (? OR visibility='applicant') ORDER BY id",
        )
        .bind(cid)
        .bind(a.is_staff())
        .fetch_all(&mut *c)
        .await?;
        let def = crate::services::definition::load_for_case(&mut c, &case).await?;
        for (at, kind, summary, data) in events {
            let data: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
            let summary = crate::cases::timeline::event_summary(&def, &kind, &data, &summary, a.is_staff());
            let event = json!({"case_id":cid,"case_number":case.number,"at":at,"kind":kind,"summary":summary});
            history.push((at, event));
        }
        cases.push(json!({"id":case.id,"number":case.number,"title":case.title,"status":case.status,"created_at":case.created_at,"links":visible_links,"documents":documents}));
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
    let chains = chains(&decisions);
    history.sort_by(|a, b| a.0.cmp(&b.0));
    let history: Vec<Value> = history.into_iter().map(|(_, e)| e).collect();
    Ok(Json(
        json!({"id":id,"reference":r.get::<String,_>("reference"),"title":r.get::<String,_>("title"),"property_ref":r.get::<String,_>("property_ref"),"created_at":r.get::<String,_>("created_at"),"cases":cases,"decisions":decisions,"chains":chains,"history":history}),
    ))
}
/// The original DA/BA type at the root of a decision's supersedes chain.
pub(crate) async fn root_type(tx: &mut SqliteConnection, mut id: i64) -> AppResult<String> {
    for _ in 0..1000 {
        let (t, parent): (String, Option<i64>) =
            sqlx::query_as("SELECT decision_type,supersedes_decision_id FROM decisions WHERE id=?")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        match parent {
            Some(p) if t == "modification_approval" => id = p,
            _ => return Ok(t),
        }
    }
    Err(AppError::internal("Decision supersedes chain is too long."))
}
/// Readable labels of every decision a submitted request names in a `decision_ref` answer (`{decision_id}` or
/// `{decision_ids}`, validated at submission), keyed by decision id: approval type, the decision's own type and its
/// case number. The ids come from the frozen answers, so every `decision_ref` field is covered.
pub async fn decision_ref_labels(
    tx: &mut SqliteConnection,
    def: &ServiceDefinition,
    answers: &Value,
) -> AppResult<Value> {
    let mut out = serde_json::Map::new();
    for f in def.fields.iter().filter(|f| f.field_type == FieldType::DecisionRef) {
        for id in answers.get(&f.key).and_then(decision_ids).unwrap_or_default() {
            if out.contains_key(&id.to_string()) {
                continue;
            }
            let row: Option<(String, Option<String>)> = sqlx::query_as(
                "SELECT d.decision_type,c.number FROM decisions d JOIN cases c ON c.id=d.case_id WHERE d.id=?",
            )
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some((decision_type, number)) = row {
                out.insert(
                    id.to_string(),
                    json!({"approval_type":root_type(tx,id).await?,"decision_type":decision_type,"case_number":number}),
                );
            }
        }
    }
    Ok(Value::Object(out))
}
/// One chain per original approval: the root DA/BA and every issued modification that names it (directly or
/// through an earlier modification). The latest approved version is current; earlier ones are superseded.
fn chains(decisions: &[super::decisions::Decision]) -> Vec<Value> {
    let issued: Vec<&super::decisions::Decision> = decisions.iter().filter(|d| d.status == "issued").collect();
    let root_of = |start: &super::decisions::Decision| -> Option<i64> {
        let modification = |d: &super::decisions::Decision| d.decision_type == "modification_approval";
        let (mut is_mod, mut id, mut parent) = (modification(start), start.id, start.supersedes_decision_id);
        for _ in 0..1000 {
            if !is_mod {
                return Some(id);
            }
            let p = issued.iter().find(|p| Some(p.id) == parent)?;
            (is_mod, id, parent) = (modification(p), p.id, p.supersedes_decision_id);
        }
        None
    };
    let mut out = vec![];
    for root in issued.iter().filter(|d| APPROVAL_TYPES.contains(&d.decision_type.as_str())) {
        let versions: Vec<&&super::decisions::Decision> =
            issued.iter().filter(|d| root_of(d) == Some(root.id)).collect();
        let approved = |d: &super::decisions::Decision| d.outcome != "refused";
        let current = versions.iter().rev().find(|d| approved(d)).map(|d| d.id);
        let list: Vec<Value> = versions
            .iter()
            .map(|d| {
                let state = if Some(d.id) == current {
                    "current"
                } else if !approved(d) {
                    "refused"
                } else {
                    "superseded"
                };
                json!({"id":d.id,"case_id":d.case_id,"decision_type":d.decision_type,"outcome":d.outcome,"issued_at":d.issued_at,"supersedes_decision_id":d.supersedes_decision_id,"output_document_version_id":d.output_document_version_id,"state":state})
            })
            .collect();
        out.push(json!({"root_decision_id":root.id,"approval_type":root.decision_type,"current_decision_id":current,"versions":list}));
    }
    out
}

/// Current approval scope of a building approval route (latest row; `confirmed` when set by staff).
pub struct Scope {
    pub approvals: Vec<String>,
    pub originals: Vec<i64>,
    pub confirmed: bool,
}
pub async fn scope(tx: &mut SqliteConnection, case_id: i64) -> AppResult<Option<Scope>> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT scope_json,source FROM building_approval_scopes WHERE case_id=? ORDER BY id DESC LIMIT 1",
    )
    .bind(case_id)
    .fetch_optional(&mut *tx)
    .await?;
    row.map(|(json, source)| {
        let v: Value = serde_json::from_str(&json)?;
        Ok(Scope {
            approvals: v["approvals"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect(),
            originals: v["originals"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect(),
            confirmed: source == "staff",
        })
    })
    .transpose()
}
async fn record_scope(
    tx: &mut SqliteConnection,
    case_id: i64,
    scope: &Value,
    actor: Option<i64>,
    reason: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO building_approval_scopes(case_id,scope_json,source,reason,set_by,set_at) VALUES(?,?,?,?,?,?)",
    )
    .bind(case_id)
    .bind(scope.to_string())
    .bind(if actor.is_some() { "staff" } else { "applicant" })
    .bind(reason)
    .bind(actor)
    .bind(crate::time::now_str())
    .execute(&mut *tx)
    .await?;
    Ok(())
}
/// Originals a modification may supersede: the confirmed scope, else every linked original (legacy cases).
async fn originals_in_scope(tx: &mut SqliteConnection, case_id: i64) -> AppResult<Vec<i64>> {
    Ok(match scope(tx, case_id).await? {
        Some(s) => s.originals,
        None => {
            sqlx::query_scalar(
                "SELECT decision_id FROM building_original_approvals WHERE case_id=? ORDER BY decision_id",
            )
            .bind(case_id)
            .fetch_all(&mut *tx)
            .await?
        }
    })
}
fn label(t: &str) -> String {
    let mut s = super::decisions::label(t);
    if let Some(first) = s.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    s
}
/// Decision-step guard: the decisions required by the case's scope (DA only, BA only, both; or one issued
/// modification per original in scope), else the step's own `decision_types`.
pub async fn decision_guard(
    tx: &mut SqliteConnection,
    case: &CaseRow,
    step: &crate::services::definition::StepDef,
) -> AppResult<Option<String>> {
    let issued = super::api::issued_decisions(tx, case.id).await?;
    let kind = approval_role(tx, case).await?;
    let scope = if kind.is_some() { scope(tx, case.id).await? } else { None };
    let Some(scope) = scope else {
        let missing: Vec<&str> = step
            .decision_types
            .iter()
            .filter(|t| !issued.iter().any(|d| &d.decision_type == *t))
            .map(String::as_str)
            .collect();
        return Ok((!missing.is_empty()).then(|| format!("Waiting for an issued decision: {}.", missing.join(", "))));
    };
    if !scope.confirmed {
        return Ok(Some(
            "Confirm the approval scope (development approval, building approval or both; or the originals being modified) with a reason before decisions."
                .into(),
        ));
    }
    let missing: Vec<String> = if kind == Some(BuildingRole::Modification) {
        let mut missing = vec![];
        for original in &scope.originals {
            let done: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM decisions WHERE case_id=? AND decision_type='modification_approval' AND status='issued' AND supersedes_decision_id=?)")
                .bind(case.id)
                .bind(original)
                .fetch_one(&mut *tx)
                .await?;
            if !done {
                missing.push(format!(
                    "modification approval of {} #{original}",
                    super::decisions::label(&root_type(tx, *original).await?)
                ));
            }
        }
        missing
    } else {
        scope
            .approvals
            .iter()
            .filter(|t| !issued.iter().any(|d| &d.decision_type == *t))
            .map(|t| super::decisions::label(t))
            .collect()
    };
    Ok((!missing.is_empty()).then(|| format!("Waiting for an issued decision: {}.", missing.join(", "))))
}
/// Checks a new draft against the scope; returns the original a modification decision supersedes.
pub async fn prepare_check(
    tx: &mut SqliteConnection,
    case: &CaseRow,
    decision_type: &str,
    target: Option<i64>,
) -> AppResult<Option<i64>> {
    let kind = approval_role(tx, case).await?;
    if decision_type == "modification_approval" {
        let originals = originals_in_scope(tx, case.id).await?;
        let target = match (target, originals.as_slice()) {
            (Some(t), _) => t,
            (None, [only]) => *only,
            (None, []) => {
                return Err(AppError::field("decision_type", "Link the request to its original approval first."));
            }
            (None, _) => {
                return Err(AppError::field(
                    "supersedes_decision_id",
                    "Choose which original approval this modification decision supersedes.",
                ));
            }
        };
        if !originals.contains(&target) {
            return Err(AppError::field(
                "supersedes_decision_id",
                "Choose an original approval that is in this request's modification scope.",
            ));
        }
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM decisions WHERE case_id=? AND decision_type='modification_approval' AND supersedes_decision_id=?)")
            .bind(case.id)
            .bind(target)
            .fetch_one(&mut *tx)
            .await?;
        if exists {
            return Err(AppError::conflict(
                "A modification decision for this original approval already exists. Edit that draft instead.",
            ));
        }
        return Ok(Some(target));
    }
    if kind == Some(BuildingRole::Project)
        && let Some(s) = scope(tx, case.id).await?
        && !s.approvals.iter().any(|t| t == decision_type)
    {
        return Err(AppError::field(
            "decision_type",
            "This approval type is not in the request's approval scope. Change the scope first, with a reason.",
        ));
    }
    Ok(None)
}
/// Issue-time rules for DA/BA/modification decisions: workflow position, fee, public exhibition and scope.
pub async fn issue_block(
    tx: &mut SqliteConnection,
    case: &CaseRow,
    d: &super::decisions::Decision,
) -> AppResult<Option<String>> {
    let kind = approval_role(tx, case).await?;
    let definition = crate::services::definition::load_for_case(tx, case).await?;
    if kind.is_some() && crate::finance::building_fees::applies(tx, case).await? {
        let at_decision = case
            .current_step
            .as_deref()
            .and_then(|key| definition.step(key))
            .is_some_and(|step| step.kind == crate::services::definition::StepKind::Decision);
        if !at_decision {
            return Ok(Some(
                "Complete fee assessment, payment, assessment and the public exhibition stage before issuing decisions."
                    .into(),
            ));
        }
    }
    if let Some(block) = crate::finance::building_fees::decision_block(tx, case).await? {
        return Ok(Some(block));
    }
    if let Some(block) = super::exhibition::case_block(tx, case.id).await? {
        return Ok(Some(block));
    }
    if let Some(block) = exhibition_unsettled(tx, case, &definition).await? {
        return Ok(Some(block));
    }
    if kind.is_some()
        && let Some(s) = scope(tx, case.id).await?
    {
        let in_scope = if d.decision_type == "modification_approval" {
            d.supersedes_decision_id.is_some_and(|o| s.originals.contains(&o))
        } else {
            s.approvals.contains(&d.decision_type)
        };
        if !s.confirmed || !in_scope {
            return Ok(Some("This decision is not in the request's confirmed approval scope.".into()));
        }
    }
    Ok(None)
}
/// A route with a public exhibition step issues approvals only once the exhibition question is settled: a closed,
/// formally terminated or reasoned withdrawal with every comment considered, a recorded "not required" decision, or
/// (cases from before those records existed) the case having already left the exhibition step through the workflow.
/// The stored step order is not trusted, so a decision step placed before the exhibition cannot issue early.
async fn exhibition_unsettled(
    tx: &mut SqliteConnection,
    case: &CaseRow,
    definition: &crate::services::definition::ServiceDefinition,
) -> AppResult<Option<String>> {
    let Some(step) =
        definition.workflow.steps.iter().find(|s| s.handler.as_deref() == Some("documents.exhibition_closed"))
    else {
        return Ok(None);
    };
    let Some(block) = super::exhibition::step_block(tx, case.id).await? else {
        return Ok(None);
    };
    let passed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_step_runs WHERE case_id=? AND step_key=? AND left_reason IN ('advanced','skipped'))",
    )
    .bind(case.id)
    .bind(&step.key)
    .fetch_one(&mut *tx)
    .await?;
    Ok((!passed).then(|| format!("The public exhibition stage must be settled before approvals are issued. {block}")))
}

#[derive(serde::Deserialize)]
pub struct ScopeInput {
    #[serde(default)]
    pub approvals: Vec<String>,
    #[serde(default)]
    pub originals: Vec<i64>,
    #[serde(default)]
    pub reason: String,
    pub expected_revision: i64,
}
/// Staff confirm or change which approvals this request covers, with a recorded reason.
pub async fn set_scope(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<ScopeInput>,
) -> AppResult<Json<Value>> {
    use crate::authz::Role;
    super::text("reason", &input.reason, 5000)?;
    let mut tx = crate::db::write_tx(&state.db).await?;
    let case = super::manage(&mut tx, &actor, id, &[Role::Intake, Role::Specialist, Role::Manager]).await?;
    if !crate::cases::workflow::is_open(&case) {
        return Err(AppError::conflict("Reopen the request before changing its approval scope."));
    }
    let kind = role_of(&mut tx, &case).await?;
    let issued = super::api::issued_decisions(&mut tx, id).await?;
    let (value, summary) = match kind {
        Some(BuildingRole::Project) => {
            let approvals: Vec<&str> =
                APPROVAL_TYPES.into_iter().filter(|t| input.approvals.iter().any(|a| a == t)).collect();
            if approvals.is_empty() || approvals.len() != input.approvals.len() {
                return Err(AppError::field("approvals", "Choose development approval, building approval or both."));
            }
            if let Some(d) = issued.iter().find(|d| {
                APPROVAL_TYPES.contains(&d.decision_type.as_str()) && !approvals.contains(&d.decision_type.as_str())
            }) {
                return Err(AppError::conflict(format!(
                    "A {} has already been issued; it stays in scope.",
                    super::decisions::label(&d.decision_type)
                )));
            }
            let names = approvals.iter().map(|t| label(t)).collect::<Vec<_>>().join(" and ");
            (json!({"approvals":approvals}), format!("Approval scope confirmed: {names}."))
        }
        Some(BuildingRole::Modification) => {
            let linked: Vec<i64> = sqlx::query_scalar(
                "SELECT decision_id FROM building_original_approvals WHERE case_id=? ORDER BY decision_id",
            )
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
            let mut originals: Vec<i64> = linked.iter().copied().filter(|o| input.originals.contains(o)).collect();
            originals.dedup();
            if originals.is_empty() || originals.len() != input.originals.len() {
                return Err(AppError::field(
                    "originals",
                    "Choose one or more original approvals named in the application.",
                ));
            }
            let modified: Vec<Option<i64>> = sqlx::query_scalar("SELECT supersedes_decision_id FROM decisions WHERE case_id=? AND decision_type='modification_approval' AND status='issued'")
                .bind(id)
                .fetch_all(&mut *tx)
                .await?;
            if modified.iter().flatten().any(|o| !originals.contains(o)) {
                return Err(AppError::conflict("An original with an issued modification decision stays in scope."));
            }
            let mut names = vec![];
            for o in &originals {
                names.push(format!("{} #{o}", label(&root_type(&mut tx, *o).await?)));
            }
            (json!({"originals":originals}), format!("Modification scope confirmed: {}.", names.join(" and ")))
        }
        Some(BuildingRole::FollowUp) | None => {
            return Err(AppError::conflict("This request has no approval scope."));
        }
    };
    crate::cases::core::bump_revision(&mut tx, id, Some(input.expected_revision)).await?;
    record_scope(&mut tx, id, &value, actor.db_id(), input.reason.trim()).await?;
    super::changed(
        &mut tx,
        actor.db_id(),
        id,
        "documents.approval_scope",
        Visibility::Applicant,
        &format!("{summary} Reason: {}", input.reason.trim()),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
/// Fee assessment, approval scope and public exhibition state of one building request.
pub async fn route(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    use crate::authz::Role;
    let mut c = state.db.acquire().await?;
    let (case, access) = super::access(&mut c, &actor, id).await?;
    let kind = approval_role(&mut c, &case).await?;
    let definition = crate::services::definition::load_for_case(&mut c, &case).await?;
    let roles = actor.roles_for_service(case.service_id);
    let open = crate::cases::workflow::is_open(&case);
    let history: Vec<(String, String, String, Option<String>, String)> = sqlx::query_as("SELECT s.scope_json,s.source,s.reason,u.display_name,s.set_at FROM building_approval_scopes s LEFT JOIN users u ON u.id=s.set_by WHERE s.case_id=? ORDER BY s.id DESC")
        .bind(id)
        .fetch_all(&mut *c)
        .await?;
    let staff = access.is_staff();
    let history: Vec<Value> = history
        .into_iter()
        .map(|(scope, source, reason, by, at)| {
            Ok(json!({"scope":serde_json::from_str::<Value>(&scope)?,"source":source,"reason":reason,"set_by":if staff { by } else { None },"set_at":at}))
        })
        .collect::<AppResult<_>>()?;
    let current = scope(&mut c, id).await?;
    let linked: Vec<i64> =
        sqlx::query_scalar("SELECT decision_id FROM building_original_approvals WHERE case_id=? ORDER BY decision_id")
            .bind(id)
            .fetch_all(&mut *c)
            .await?;
    let mut originals = vec![];
    for o in linked {
        let (case_id, number, issued_at): (i64, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT d.case_id,c.number,d.issued_at FROM decisions d JOIN cases c ON c.id=d.case_id WHERE d.id=?",
        )
        .bind(o)
        .fetch_one(&mut *c)
        .await?;
        let modified_by: Option<i64> = sqlx::query_scalar("SELECT id FROM decisions WHERE case_id=? AND decision_type='modification_approval' AND supersedes_decision_id=? ORDER BY id DESC LIMIT 1")
            .bind(id)
            .bind(o)
            .fetch_optional(&mut *c)
            .await?;
        originals.push(json!({"decision_id":o,"approval_type":root_type(&mut c,o).await?,"case_id":case_id,"case_number":number,"issued_at":issued_at,"in_scope":current.as_ref().is_none_or(|s| s.originals.contains(&o)),"modification_decision_id":modified_by}));
    }
    let exhibition_step =
        definition.workflow.steps.iter().any(|s| s.handler.as_deref() == Some("documents.exhibition_closed"));
    let fee = crate::finance::building_fees::view(&mut c, &actor, &case, access).await?;
    // The scope can change only while the decisions it requires are still outstanding.
    let decisions_pending =
        match definition.workflow.steps.iter().find(|s| s.kind == crate::services::definition::StepKind::Decision) {
            Some(step) => decision_guard(&mut c, &case, step).await?.is_some(),
            None => false,
        };
    Ok(Json(json!({
        "route": kind.map(BuildingRole::as_str),
        "revision": case.revision,
        "scope": current.as_ref().map(|s| json!({"approvals":s.approvals,"originals":s.originals,"confirmed":s.confirmed})),
        "scope_history": history,
        "originals": originals,
        "fee": fee,
        "exhibition_step": exhibition_step,
        "exhibition": if exhibition_step || kind.is_some() { super::exhibition::case_view(&mut c, id, staff).await? } else { Value::Null },
        "can_scope": kind.is_some() && open && decisions_pending && access.can_manage() && roles.iter().any(|r| matches!(r, Role::Intake | Role::Specialist | Role::Manager)),
        "can_exhibit": open && access.can_manage() && roles.iter().any(|r| matches!(r, Role::Specialist | Role::Manager)),
    })))
}
