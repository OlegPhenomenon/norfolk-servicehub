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
/// Building approval routes. Kept on slugs inside slice A; `role_of` (project/modification) replaces this match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteKind {
    Project,
    Modification,
}
impl RouteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RouteKind::Project => "project",
            RouteKind::Modification => "modification",
        }
    }
}
pub async fn route_kind(tx: &mut SqliteConnection, case: &CaseRow) -> AppResult<Option<RouteKind>> {
    if case.module != "building" {
        return Ok(None);
    }
    let slug: String =
        sqlx::query_scalar("SELECT slug FROM services WHERE id=?").bind(case.service_id).fetch_one(&mut *tx).await?;
    Ok(match slug.as_str() {
        "development-application" => Some(RouteKind::Project),
        "modify-approval" => Some(RouteKind::Modification),
        _ => None,
    })
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
        "modify-approval" => {
            let ids = answers
                .get("original_approval")
                .and_then(decision_ids)
                .filter(|ids| !ids.is_empty())
                .ok_or_else(|| AppError::field("original_approval", "Choose an issued approval."))?;
            let mut project = None;
            let mut originals = vec![];
            for id in &ids {
                let (original,p):(i64,Option<i64>)=sqlx::query_as(&format!("SELECT d.case_id,c.building_project_id FROM decisions d JOIN cases c ON c.id=d.case_id WHERE d.id=? AND {CURRENT_APPROVAL}")).bind(id).fetch_optional(&mut *tx).await?.ok_or_else(||AppError::field("original_approval","Choose a current issued approval you can access."))?;
                if !applicant_access(tx, actor, case, original).await? {
                    return Err(AppError::field(
                        "original_approval",
                        "Choose an approval visible to you as an applicant or representative.",
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
    let rows=sqlx::query(&format!("SELECT d.id,d.case_id,d.decision_type,d.issued_at,c.number,c.building_project_id,c.property_ref FROM decisions d JOIN cases c ON c.id=d.case_id WHERE {CURRENT_APPROVAL} ORDER BY d.id DESC")).fetch_all(&mut *c).await?;
    let mut out = vec![];
    for r in rows {
        let id: i64 = r.get("case_id");
        if crate::authz::case_access(&mut c, &actor, id).await? == CaseAccess::Applicant {
            let approval_type = root_type(&mut c, r.get("id")).await?;
            out.push(json!({"id":r.get::<i64,_>("id"),"decision_type":r.get::<String,_>("decision_type"),"approval_type":approval_type,"issued_at":r.get::<String,_>("issued_at"),"case_number":r.get::<Option<String>,_>("number"),"project_id":r.get::<Option<i64>,_>("building_project_id"),"property_ref":r.get::<Option<String>,_>("property_ref")}));
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
    let chains = chains(&decisions);
    Ok(Json(
        json!({"id":id,"reference":r.get::<String,_>("reference"),"title":r.get::<String,_>("title"),"property_ref":r.get::<String,_>("property_ref"),"created_at":r.get::<String,_>("created_at"),"cases":cases,"decisions":decisions,"chains":chains}),
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
    let kind = route_kind(tx, case).await?;
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
    let missing: Vec<String> = if kind == Some(RouteKind::Modification) {
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
    let kind = route_kind(tx, case).await?;
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
    if kind == Some(RouteKind::Project)
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
    let kind = route_kind(tx, case).await?;
    if kind.is_some() && crate::finance::building_fees::applies(tx, case).await? {
        let definition = crate::services::definition::load_for_case(tx, case).await?;
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
    let kind =
        route_kind(&mut tx, &case).await?.ok_or_else(|| AppError::conflict("This request has no approval scope."))?;
    let issued = super::api::issued_decisions(&mut tx, id).await?;
    let (value, summary) = match kind {
        RouteKind::Project => {
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
        RouteKind::Modification => {
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
    let kind = route_kind(&mut c, &case).await?;
    let definition = crate::services::definition::load_for_case(&mut c, &case).await?;
    let roles = actor.roles_for_service(case.service_id);
    let open = crate::cases::workflow::is_open(&case);
    let history: Vec<(String, String, String, Option<String>, String)> = sqlx::query_as("SELECT s.scope_json,s.source,s.reason,u.display_name,s.set_at FROM building_approval_scopes s LEFT JOIN users u ON u.id=s.set_by WHERE s.case_id=? ORDER BY s.id DESC")
        .bind(id)
        .fetch_all(&mut *c)
        .await?;
    let history: Vec<Value> = history
        .into_iter()
        .map(|(scope, source, reason, by, at)| {
            Ok(json!({"scope":serde_json::from_str::<Value>(&scope)?,"source":source,"reason":reason,"set_by":by,"set_at":at}))
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
    Ok(Json(json!({
        "route": kind.map(RouteKind::as_str),
        "revision": case.revision,
        "scope": current.as_ref().map(|s| json!({"approvals":s.approvals,"originals":s.originals,"confirmed":s.confirmed})),
        "scope_history": history,
        "originals": originals,
        "fee": fee,
        "exhibition_step": exhibition_step,
        "exhibition": if exhibition_step || kind.is_some() { super::exhibition::case_view(&mut c, id).await? } else { Value::Null },
        "can_scope": kind.is_some() && open && access.can_manage() && roles.iter().any(|r| matches!(r, Role::Intake | Role::Specialist | Role::Manager)),
        "can_exhibit": open && access.can_manage() && roles.iter().any(|r| matches!(r, Role::Specialist | Role::Manager)),
    })))
}
