use crate::{
    auth::Actor,
    authz::Role,
    cases::core::{CaseRow, Visibility},
    db::write_tx,
    error::{AppError, AppResult},
    state::AppState,
    time,
    web::{Json, Path},
};
use axum::extract::State;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
const TYPES: &[&str] =
    &["development_approval", "building_approval", "modification_approval", "planning_certificate", "service_response"];
pub fn label(t: &str) -> String {
    t.replace('_', " ")
}
#[derive(Serialize, sqlx::FromRow)]
pub struct Decision {
    pub id: i64,
    pub case_id: i64,
    pub decision_type: String,
    pub outcome: String,
    pub reasons: String,
    pub conditions: Option<String>,
    pub status: String,
    pub template_id: Option<i64>,
    pub prepared_by: i64,
    pub approved_by: Option<i64>,
    pub returned_reason: Option<String>,
    pub issued_at: Option<String>,
    pub supersedes_decision_id: Option<i64>,
    pub output_document_version_id: Option<i64>,
    #[sqlx(skip)]
    pub evidence: Vec<Evidence>,
}
#[derive(Serialize, sqlx::FromRow)]
pub struct Evidence {
    pub id: i64,
    pub title: String,
    pub version: i64,
}
#[derive(Serialize, sqlx::FromRow)]
pub struct Template {
    pub id: i64,
    pub code: String,
    pub version: i64,
    pub name: String,
    pub decision_type: String,
    pub body_template: String,
}
#[derive(Deserialize)]
pub struct Input {
    pub decision_type: String,
    pub outcome: String,
    pub reasons: String,
    #[serde(default)]
    pub conditions: String,
    pub template_id: i64,
    pub evidence_version_ids: Option<Vec<i64>>,
    pub expected_revision: i64,
}
#[derive(Deserialize)]
pub struct ActionInput {
    pub expected_revision: i64,
    pub reason: Option<String>,
}
pub async fn templates(State(state): State<AppState>, actor: Actor) -> AppResult<Json<Vec<Template>>> {
    actor.require_any_role(&[Role::Specialist, Role::Manager])?;
    Ok(Json(sqlx::query_as("SELECT id,code,version,name,decision_type,body_template FROM decision_templates WHERE active=1 ORDER BY decision_type,version DESC").fetch_all(&state.db).await?))
}
pub(crate) async fn load(tx: &mut SqliteConnection, id: i64, case: i64) -> AppResult<Decision> {
    sqlx::query_as("SELECT * FROM decisions WHERE id=? AND case_id=?")
        .bind(id)
        .bind(case)
        .fetch_optional(tx)
        .await?
        .ok_or_else(AppError::not_found)
}
pub(crate) async fn evidence(tx: &mut SqliteConnection, id: i64) -> AppResult<Vec<Evidence>> {
    Ok(sqlx::query_as("SELECT v.id,d.title,v.version FROM decision_evidence e JOIN document_versions v ON v.id=e.document_version_id JOIN documents d ON d.id=v.document_id WHERE e.decision_id=? ORDER BY d.id,v.version").bind(id).fetch_all(tx).await?)
}
pub(crate) async fn authority(tx: &mut SqliteConnection, actor: &Actor, case: &CaseRow, t: &str) -> AppResult<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM decision_authorities WHERE user_id=? AND decision_type=? AND revoked_at IS NULL AND (service_id IS NULL OR service_id=?))").bind(actor.user_id).bind(t).bind(case.service_id).fetch_one(tx).await?)
}
pub async fn list(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let mut c = state.db.acquire().await?;
    let (case, a) = super::access(&mut c, &actor, id).await?;
    let mut rows: Vec<Decision> =
        sqlx::query_as("SELECT * FROM decisions WHERE case_id=? AND (? OR status='issued') ORDER BY id")
            .bind(id)
            .bind(a.is_staff())
            .fetch_all(&mut *c)
            .await?;
    for d in &mut rows {
        d.evidence = evidence(&mut c, d.id).await?;
    }
    let mut authorities = Vec::new();
    if a.is_staff() {
        for t in TYPES {
            if authority(&mut c, &actor, &case, t).await? {
                authorities.push(*t);
            }
        }
    }
    let editable = super::uploads::editable(&case).is_ok();
    let roles = actor.roles_for_service(case.service_id);
    let can_prepare = a.can_manage() && roles.iter().any(|r| matches!(r, Role::Specialist | Role::Manager));
    let can_comment = a.can_manage()
        && roles.iter().any(|r| matches!(r, Role::Intake | Role::Specialist | Role::Manager | Role::ComplaintsOfficer));
    let requirements: String = sqlx::query_scalar("SELECT definition_json FROM service_versions WHERE id=?")
        .bind(case.service_version_id)
        .fetch_one(&mut *c)
        .await?;
    let definition: serde_json::Value = serde_json::from_str(&requirements)?;
    let role = super::building::role_of(&mut c, &case).await?;
    let allowed: Vec<&str> = crate::services::validation::decision_types(&case.module)
        .into_iter()
        .filter(|t| super::building::role_permits(role, t))
        .collect();
    Ok(Json(
        serde_json::json!({"editable":editable,"can_upload":editable && super::uploads::writable(a).is_ok(),"can_comment":editable && can_comment,"can_prepare":can_prepare,"document_requirements":definition.get("documents").cloned().unwrap_or_else(||serde_json::json!([])),"items":rows,"authorities":authorities,"staff":a.is_staff(),"revision":case.revision,"building_project_id":case.building_project_id,"allowed_decision_types":allowed}),
    ))
}
async fn validate(tx: &mut SqliteConnection, case: &CaseRow, input: &Input) -> AppResult<Vec<i64>> {
    if !TYPES.contains(&input.decision_type.as_str()) {
        return Err(AppError::field("decision_type", "Choose an approval or planning certificate."));
    }
    let permitted = crate::services::validation::decision_types(&case.module).contains(&input.decision_type.as_str());
    if !permitted {
        return Err(AppError::field("decision_type", "This decision type does not match the service."));
    }
    if !super::building::role_permits(super::building::role_of(tx, case).await?, &input.decision_type) {
        return Err(AppError::field("decision_type", "This approval does not match the request."));
    }
    if !matches!(input.outcome.as_str(), "approved" | "approved_with_conditions" | "refused") {
        return Err(AppError::field("outcome", "Choose an outcome."));
    }
    super::text("reasons", &input.reasons, 20000)?;
    if input.conditions.len() > 20000 {
        return Err(AppError::field("conditions", "Conditions are too long."));
    }
    if input.outcome == "approved_with_conditions" {
        super::text("conditions", &input.conditions, 20000)?;
    }
    let template: Option<(String, String)> =
        sqlx::query_as("SELECT decision_type,body_template FROM decision_templates WHERE id=? AND active=1")
            .bind(input.template_id)
            .fetch_optional(&mut *tx)
            .await?;
    let (t, body) = template.ok_or_else(|| AppError::field("template_id", "Choose an active template."))?;
    if t != input.decision_type {
        return Err(AppError::field("template_id", "Choose a template for this decision type."));
    }
    validate_template(&body)?;
    let mut ids = if let Some(ids) = &input.evidence_version_ids {
        ids.clone()
    } else {
        sqlx::query_scalar("SELECT v.id FROM documents d JOIN document_versions v ON v.document_id=d.id WHERE d.case_id=? AND d.visibility='applicant' AND d.disposed_at IS NULL AND d.category NOT IN ('decision','letter','certificate') AND v.version=(SELECT MAX(v2.version) FROM document_versions v2 WHERE v2.document_id=d.id)").bind(case.id).fetch_all(&mut *tx).await?
    };
    ids.sort_unstable();
    ids.dedup();
    if ids.len() > 100 {
        return Err(AppError::field("evidence_version_ids", "Choose at most 100 versions."));
    }
    for id in &ids {
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM document_versions v JOIN documents d ON d.id=v.document_id WHERE v.id=? AND d.case_id=? AND d.visibility='applicant' AND d.disposed_at IS NULL)").bind(id).bind(case.id).fetch_one(&mut *tx).await?;
        if !valid {
            return Err(AppError::field(
                "evidence_version_ids",
                "Evidence must be an applicant-visible version from this case.",
            ));
        }
    }
    Ok(ids)
}
async fn set_evidence(tx: &mut SqliteConnection, id: i64, ids: Vec<i64>) -> AppResult<()> {
    sqlx::query("DELETE FROM decision_evidence WHERE decision_id=?").bind(id).execute(&mut *tx).await?;
    for vid in ids {
        sqlx::query("INSERT INTO decision_evidence(decision_id,document_version_id) VALUES(?,?)")
            .bind(id)
            .bind(vid)
            .execute(&mut *tx)
            .await?;
    }
    Ok(())
}
pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Path(case_id): Path<i64>,
    Json(input): Json<Input>,
) -> AppResult<Json<serde_json::Value>> {
    let mut tx = write_tx(&state.db).await?;
    let case = super::manage(&mut tx, &actor, case_id, &[Role::Specialist, Role::Manager]).await?;
    let ids = validate(&mut tx, &case, &input).await?;
    crate::cases::core::bump_revision(&mut tx, case_id, Some(input.expected_revision)).await?;
    let supersedes: Option<i64> = if input.decision_type == "modification_approval" {
        sqlx::query_scalar("SELECT decision_id FROM building_original_approvals WHERE case_id=?")
            .bind(case_id)
            .fetch_optional(&mut *tx)
            .await?
    } else {
        None
    };
    if input.decision_type == "modification_approval" && supersedes.is_none() {
        return Err(AppError::field("decision_type", "Link the request to its original approval first."));
    }
    let id:i64=sqlx::query_scalar("INSERT INTO decisions(case_id,decision_type,outcome,reasons,conditions,status,template_id,prepared_by,supersedes_decision_id,created_at) VALUES(?,?,?,?,?,'draft',?,?,?,?) RETURNING id").bind(case_id).bind(&input.decision_type).bind(input.outcome).bind(input.reasons).bind(input.conditions).bind(input.template_id).bind(actor.user_id).bind(supersedes).bind(time::now_str()).fetch_one(&mut *tx).await?;
    set_evidence(&mut tx, id, ids).await?;
    super::changed(
        &mut tx,
        actor.db_id(),
        case_id,
        "documents.decision_prepare",
        Visibility::Staff,
        &format!("Prepared a draft {}.", label(&input.decision_type)),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({"id":id})))
}
pub async fn update(
    State(state): State<AppState>,
    actor: Actor,
    Path((case_id, id)): Path<(i64, i64)>,
    Json(input): Json<Input>,
) -> AppResult<Json<serde_json::Value>> {
    let mut tx = write_tx(&state.db).await?;
    let case = super::manage(&mut tx, &actor, case_id, &[Role::Specialist, Role::Manager]).await?;
    let d = load(&mut tx, id, case_id).await?;
    if !matches!(d.status.as_str(), "draft" | "returned") {
        return Err(AppError::conflict("Only draft or returned decisions can be edited."));
    }
    if input.decision_type != d.decision_type {
        return Err(AppError::field("decision_type", "Prepare a separate decision for a different approval type."));
    }
    let ids = validate(&mut tx, &case, &input).await?;
    crate::cases::core::bump_revision(&mut tx, case_id, Some(input.expected_revision)).await?;
    sqlx::query("UPDATE decisions SET outcome=?,reasons=?,conditions=?,template_id=? WHERE id=?")
        .bind(input.outcome)
        .bind(input.reasons)
        .bind(input.conditions)
        .bind(input.template_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    set_evidence(&mut tx, id, ids).await?;
    super::changed(
        &mut tx,
        actor.db_id(),
        case_id,
        "documents.decision_edit",
        Visibility::Staff,
        "Updated the draft decision and its evidence versions.",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({"id":id})))
}
pub async fn action(
    State(state): State<AppState>,
    actor: Actor,
    Path((case_id, id, action)): Path<(i64, i64, String)>,
    Json(input): Json<ActionInput>,
) -> AppResult<Json<serde_json::Value>> {
    let mut tx = write_tx(&state.db).await?;
    let (case, a) = super::access(&mut tx, &actor, case_id).await?;
    if !a.is_staff() {
        return Err(AppError::not_found());
    }
    let d = load(&mut tx, id, case_id).await?;
    if !crate::cases::workflow::is_open(&case) {
        return Err(AppError::conflict("Reopen the request before acting on a decision."));
    }
    if action == "submit" {
        super::manage(&mut tx, &actor, case_id, &[Role::Specialist, Role::Manager]).await?;
        if !matches!(d.status.as_str(), "draft" | "returned") {
            return Err(AppError::conflict("Only draft or returned decisions can be submitted."));
        }
    } else if matches!(action.as_str(), "return" | "issue") {
        if !authority(&mut tx, &actor, &case, &d.decision_type).await? {
            return Err(AppError::forbidden_msg("An active authority for this decision type is required."));
        }
        if d.status != "pending_approval" {
            return Err(AppError::conflict("Submit the decision for approval first."));
        }
    } else {
        return Err(AppError::not_found());
    }
    crate::cases::core::bump_revision(&mut tx, case_id, Some(input.expected_revision)).await?;
    match action.as_str() {
        "submit" => {
            sqlx::query("UPDATE decisions SET status='pending_approval',returned_reason=NULL WHERE id=?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            let users:Vec<i64>=sqlx::query_scalar("SELECT DISTINCT a.user_id FROM decision_authorities a JOIN users u ON u.id=a.user_id WHERE a.decision_type=? AND a.revoked_at IS NULL AND (a.service_id IS NULL OR a.service_id=?) AND u.is_active=1").bind(&d.decision_type).bind(case.service_id).fetch_all(&mut *tx).await?;
            for user in users {
                let Some(recipient) = Actor::load_recipient(&mut tx, user).await? else { continue };
                if crate::authz::case_access(&mut tx, &recipient, case_id).await?.is_staff() {
                    crate::notify::send(
                        &mut tx,
                        crate::notify::Notice {
                            user_id: Some(user),
                            email: None,
                            phone: None,
                            case_id: Some(case_id),
                            subject: format!("{} needs your approval", label(&d.decision_type)),
                            body: "Review the draft and its exact evidence versions.".into(),
                            link: Some(format!("/staff/cases/{case_id}?tab=documents.decisions")),
                        },
                    )
                    .await?;
                }
            }
        }
        "return" => {
            let reason = input.reason.as_deref().unwrap_or("");
            super::text("reason", reason, 5000)?;
            sqlx::query("UPDATE decisions SET status='returned',returned_reason=? WHERE id=?")
                .bind(reason)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            crate::notify::send(
                &mut tx,
                crate::notify::Notice {
                    user_id: Some(d.prepared_by),
                    email: None,
                    phone: None,
                    case_id: Some(case_id),
                    subject: "Decision returned for changes".into(),
                    body: reason.into(),
                    link: Some(format!("/staff/cases/{case_id}?tab=documents.decisions")),
                },
            )
            .await?;
        }
        "issue" => {
            if d.decision_type == "planning_certificate" {
                let definition = crate::services::definition::load_for_case(&mut tx, &case).await?;
                let at_decision = case
                    .current_step
                    .as_deref()
                    .and_then(|key| definition.step(key))
                    .is_some_and(|step| step.kind == crate::services::definition::StepKind::Decision);
                if !at_decision || !crate::finance::api::case_settled(&mut tx, case.id).await? {
                    return Err(AppError::conflict(
                        "Complete payment and specialist preparation before issuing the planning certificate.",
                    ));
                }
            }
            issue_document(&mut tx, &state, &actor, &case, d).await?;
            crate::deadlines::api::on_trigger(&mut tx, case.id, "decision_issued").await?;
            crate::records::api::on_decision_issued(&mut tx, case.id, id).await?;
            crate::cases::workflow::try_auto_advance(&mut tx, &state, case.id).await?;
        }
        _ => unreachable!(),
    }
    super::changed(
        &mut tx,
        actor.db_id(),
        case_id,
        &format!("documents.decision_{action}"),
        if action == "issue" { Visibility::Applicant } else { Visibility::Staff },
        &format!(
            "Decision {id}: {}.",
            match action.as_str() {
                "submit" => "submitted for approval",
                "return" => "returned for changes",
                _ => "issued — download it",
            }
        ),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({"id":id})))
}
pub(crate) async fn issue_document(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: &CaseRow,
    d: Decision,
) -> AppResult<()> {
    let current = crate::cases::core::load_case(tx, case.id).await?;
    if !crate::cases::workflow::is_open(&current) || d.status != "pending_approval" {
        return Err(AppError::conflict("An open request and pending approval are required for issuance."));
    }
    if actor.user_id == d.prepared_by {
        return Err(AppError::forbidden_msg("The approver must be different from the preparer."));
    }
    if !authority(tx, actor, case, &d.decision_type).await? {
        return Err(AppError::forbidden_msg("An active authority for this decision type is required."));
    }
    if d.status != "pending_approval" {
        return Err(AppError::conflict("Submit the decision for approval first."));
    }
    let body: String = sqlx::query_scalar("SELECT body_template FROM decision_templates WHERE id=?")
        .bind(d.template_id)
        .fetch_one(&mut *tx)
        .await?;
    let e = evidence(tx, d.id).await?;
    let evidence =
        e.iter().map(|v| format!("{} v{} (version ID {})", v.title, v.version, v.id)).collect::<Vec<_>>().join("\n");
    let name: String =
        sqlx::query_scalar("SELECT name FROM services WHERE id=?").bind(case.service_id).fetch_one(&mut *tx).await?;
    let date = crate::time::local_date(state.now()).to_string();
    let values = [
        ("case_number", case.number.clone().unwrap_or_default()),
        ("applicant_name", case.applicant_name.clone()),
        ("property_ref", case.property_ref.clone().unwrap_or_default()),
        ("service_name", name),
        ("decision_date", date.clone()),
        ("conditions", d.conditions.clone().unwrap_or_default()),
        ("reasons", d.reasons.clone()),
        ("evidence_list", evidence.clone()),
        ("decision_type_label", label(&d.decision_type)),
        ("approver_name", actor.display_name.clone()),
    ];
    let rendered = render_template(&body, &values)?;
    let mut sections = vec![
        ("Decision", format!("Outcome: {}\n{rendered}", label(&d.outcome))),
        ("Conditions", d.conditions.clone().unwrap_or_default()),
        ("Exact evidence versions", evidence),
    ];
    if d.decision_type == "planning_certificate" {
        let answers: Option<String> =
            sqlx::query_scalar("SELECT answers_json FROM submissions WHERE case_id=? ORDER BY id DESC LIMIT 1")
                .bind(case.id)
                .fetch_optional(&mut *tx)
                .await?;
        let a: serde_json::Value = answers.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        sections.push((
            "Section 98 Planning Act 2002",
            format!(
                "Property (Portion/Lot): {}\nRequested sections: {}\nInformation as recorded on {date}",
                a.get("property_ref")
                    .and_then(|v| v.as_str())
                    .or(case.property_ref.as_deref())
                    .unwrap_or("See request"),
                a.get("sections")
                    .or_else(|| a.get("certificate_sections"))
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "See request".into())
            ),
        ));
    }
    let title = label(&d.decision_type);
    let pdf = crate::pdf::simple_document(
        &title,
        &[("Case", case.number.clone().unwrap_or_default()), ("Issued", date)],
        &sections,
    );
    let (doc, vid) =
        super::api::attach_generated(tx, state, case.id, "decision", &title, Visibility::Applicant, pdf, actor.db_id())
            .await?;
    sqlx::query("UPDATE decisions SET status='issued',approved_by=?,issued_at=?,output_document_id=?,output_document_version_id=? WHERE id=? AND status='pending_approval'").bind(actor.user_id).bind(state.now().to_rfc3339()).bind(doc).bind(vid).bind(d.id).execute(&mut *tx).await?;
    super::notify_applicant(
        tx,
        case,
        &format!("Your {title} has been issued — download it"),
        &format!(
            "{title}\nOutcome: {}\n{}\nConditions: {}",
            d.outcome,
            d.reasons,
            d.conditions.as_deref().unwrap_or("")
        ),
    )
    .await?;
    Ok(())
}
const PLACEHOLDERS: &[&str] = &[
    "case_number",
    "applicant_name",
    "property_ref",
    "service_name",
    "decision_date",
    "conditions",
    "reasons",
    "evidence_list",
    "decision_type_label",
    "approver_name",
];
pub(crate) fn validate_template(body: &str) -> AppResult<()> {
    let mut rest = body;
    while let Some((prefix, tail)) = rest.split_once("{{") {
        if prefix.contains("}}") {
            return Err(AppError::field("template_id", "The template has an unmatched placeholder."));
        }
        let (key, next) = tail
            .split_once("}}")
            .ok_or_else(|| AppError::field("template_id", "The template has an unfinished placeholder."))?;
        if !PLACEHOLDERS.contains(&key) {
            return Err(AppError::field("template_id", "The template contains an unsupported placeholder."));
        }
        rest = next;
    }
    if rest.contains("}}") {
        return Err(AppError::field("template_id", "The template has an unmatched placeholder."));
    }
    Ok(())
}
pub(crate) fn render_template(body: &str, values: &[(&str, String)]) -> AppResult<String> {
    validate_template(body)?;
    let mut out = String::new();
    let mut rest = body;
    while let Some((prefix, tail)) = rest.split_once("{{") {
        out.push_str(prefix);
        let (key, next) = tail.split_once("}}").expect("validated placeholder");
        out.push_str(values.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str()).unwrap_or(""));
        rest = next;
    }
    out.push_str(rest);
    Ok(out)
}
#[derive(Deserialize)]
pub struct LetterInput {
    letter_type: String,
    title: String,
    body: String,
    expected_revision: i64,
}
pub async fn letter(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(input): Json<LetterInput>,
) -> AppResult<Json<serde_json::Value>> {
    let mut tx = write_tx(&state.db).await?;
    super::manage(&mut tx, &actor, id, &[Role::Manager, Role::Specialist, Role::Intake, Role::ComplaintsOfficer])
        .await?;
    crate::cases::core::bump_revision(&mut tx, id, Some(input.expected_revision)).await?;
    let doc =
        super::api::issue_letter(&mut tx, &state, &actor, id, &input.letter_type, &input.title, &input.body).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({"document_id":doc})))
}
