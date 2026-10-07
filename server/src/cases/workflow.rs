//! Workflow commands operate only on the case's own definition and call platform hooks.
use super::{
    core::{self, CaseRow, Visibility},
    record,
};
use crate::{
    auth::Actor,
    authz::{self, CaseAccess, Role},
    db, deadlines,
    error::{AppError, AppResult},
    hooks,
    services::definition::{self, ServiceDefinition, StepDef, StepKind},
    state::AppState,
    time,
    web::{Json, Path},
};
use axum::{
    Router,
    extract::State,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
pub fn routes() -> Router<AppState> {
    Router::new().route("/api/cases/{id}", get(detail)).route("/api/cases/{id}/actions/{action}", post(action))
}
pub fn is_open(c: &CaseRow) -> bool {
    matches!(c.status.as_str(), "submitted" | "in_progress" | "waiting_on_applicant")
}
pub fn can_step(actor: &Actor, c: &CaseRow, step: &StepDef) -> bool {
    actor.is_staff() && step.role.is_some_and(|r| actor.roles_for_service(c.service_id).contains(&r))
}
pub fn allowed_actions(actor: &Actor, c: &CaseRow, access: CaseAccess, def: &ServiceDefinition) -> Vec<&'static str> {
    let mut actions = vec![];
    if access == CaseAccess::Applicant && is_open(c) {
        actions.push("withdraw");
    }
    if access.is_staff()
        && is_open(c)
        && def.step(c.current_step.as_deref().unwrap_or("")).is_some_and(|s| can_step(actor, c, s))
    {
        if c.status != "waiting_on_applicant" {
            actions.push("advance");
            if def.step(c.current_step.as_deref().unwrap_or("")).is_some_and(|s| s.optional) {
                actions.push("skip");
            }
        }
        actions.extend(["request-info", "refuse"]);
    }
    if access.can_manage() {
        if is_open(c) {
            actions.extend(["cancel", "close-duplicate", "assign", "escalate"]);
        } else if c.status != "draft" && actor.roles_for_service(c.service_id).contains(&Role::Manager) {
            actions.push("reopen");
        }
    }
    actions
}
pub async fn enter_step(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    c: &CaseRow,
    step: &StepDef,
) -> AppResult<()> {
    let run: i64 =
        sqlx::query_scalar("INSERT INTO workflow_step_runs(case_id,step_key,entered_at) VALUES (?,?,?) RETURNING id")
            .bind(c.id)
            .bind(&step.key)
            .bind(time::fmt(state.now()))
            .fetch_one(&mut *tx)
            .await?;
    deadlines::api::on_trigger_at(tx, c.id, &format!("step:{}", step.key), state.now()).await?;
    hooks::on_step_entered(tx, state, actor, c, step, run).await
}
pub async fn advance(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case_id: i64,
    expected_revision: i64,
    note: Option<String>,
) -> AppResult<CaseRow> {
    let (case, access) = authz::require_case(tx, actor, case_id).await?;
    let def = definition::load_for_case(tx, &case).await?;
    if !allowed_actions(actor, &case, access, &def).contains(&"advance") {
        return Err(AppError::forbidden());
    }
    transition(
        tx,
        state,
        actor,
        &case,
        &def,
        expected_revision,
        false,
        note.as_deref().unwrap_or("Staff completed this step."),
    )
    .await
}
#[allow(clippy::too_many_arguments)]
async fn transition(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: &CaseRow,
    def: &ServiceDefinition,
    revision: i64,
    skip: bool,
    reason: &str,
) -> AppResult<CaseRow> {
    let index = def
        .workflow
        .steps
        .iter()
        .position(|s| Some(&s.key) == case.current_step.as_ref())
        .ok_or_else(|| AppError::conflict("No active workflow step."))?;
    let step = &def.workflow.steps[index];
    core::bump_revision(tx, case.id, Some(revision)).await?;
    if !skip {
        if step.kind == StepKind::Decision {
            let decisions = crate::documents::api::issued_decisions(tx, case.id).await?;
            if decisions.iter().any(|d| {
                d.status == "issued" && d.outcome == "refused" && step.decision_types.contains(&d.decision_type)
            }) {
                close(tx, state, actor, case.id, "refused", "The approval decision was refused.").await?;
                return core::load_case(tx, case.id).await;
            }
        }
        if let Some(reason) = hooks::step_guard(tx, case, step).await? {
            return Err(AppError::conflict(reason));
        }
    }
    sqlx::query("UPDATE workflow_step_runs SET left_at=?,left_reason=? WHERE case_id=? AND left_at IS NULL")
        .bind(time::fmt(state.now()))
        .bind(if skip { "skipped" } else { "advanced" })
        .bind(case.id)
        .execute(&mut *tx)
        .await?;
    let next =
        def.workflow.steps.get(index + 1).ok_or_else(|| AppError::conflict("This is the final workflow step."))?;
    sqlx::query("UPDATE cases SET current_step=?,status='in_progress',updated_at=? WHERE id=?")
        .bind(&next.key)
        .bind(time::fmt(state.now()))
        .bind(case.id)
        .execute(&mut *tx)
        .await?;
    record(
        tx,
        actor,
        case.id,
        if skip { "step.skipped" } else { "step.changed" },
        Visibility::Applicant,
        &format!("{} {}. Next: {}.", step.label, if skip { "skipped" } else { "completed" }, next.applicant_label),
        json!({"from":step.key,"to":next.key,"reason":reason}),
    )
    .await?;
    let updated = core::load_case(tx, case.id).await?;
    enter_step(tx, state, actor, &updated, next).await?;
    if next.kind == StepKind::Complete {
        close(tx, state, actor, case.id, "completed", reason).await?;
    }
    core::reindex_search(tx, case.id).await?;
    core::load_case(tx, case.id).await
}
pub async fn try_auto_advance(tx: &mut SqliteConnection, state: &AppState, case_id: i64) -> AppResult<()> {
    let case = core::load_case(tx, case_id).await?;
    if !matches!(case.status.as_str(), "submitted" | "in_progress") {
        return Ok(());
    }
    let def = definition::load_for_case(tx, &case).await?;
    let Some(step) = def.step(case.current_step.as_deref().unwrap_or("")) else {
        return Ok(());
    };
    if !matches!(step.kind, StepKind::Payment | StepKind::Task | StepKind::Module | StepKind::Decision) {
        return Ok(());
    }
    let refused = if step.kind == StepKind::Decision {
        crate::documents::api::issued_decisions(tx, case_id)
            .await?
            .iter()
            .any(|d| d.status == "issued" && d.outcome == "refused" && step.decision_types.contains(&d.decision_type))
    } else {
        false
    };
    if refused || hooks::step_guard(tx, &case, step).await?.is_none() {
        transition(
            tx,
            state,
            &Actor::system(),
            &case,
            &def,
            case.revision,
            false,
            "Automatically advanced after the required action completed.",
        )
        .await?;
    }
    Ok(())
}
pub async fn close(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case_id: i64,
    outcome: &str,
    reason: &str,
) -> AppResult<()> {
    if !["completed", "refused", "withdrawn", "cancelled", "closed_duplicate"].contains(&outcome) {
        return Err(AppError::field("outcome", "Choose a valid outcome."));
    }
    sqlx::query("UPDATE workflow_step_runs SET left_at=?,left_reason=? WHERE case_id=? AND left_at IS NULL")
        .bind(time::fmt(state.now()))
        .bind(match outcome {
            "refused" => "refused",
            "withdrawn" => "withdrawn",
            "completed" => "advanced",
            _ => "cancelled",
        })
        .bind(case_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE cases SET status=?,closed_at=?,updated_at=? WHERE id=?")
        .bind(outcome)
        .bind(time::fmt(state.now()))
        .bind(time::fmt(state.now()))
        .bind(case_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE case_messages SET resolved_at=? WHERE case_id=? AND requires_response=1 AND resolved_at IS NULL",
    )
    .bind(time::fmt(state.now()))
    .bind(case_id)
    .execute(&mut *tx)
    .await?;
    if matches!(outcome, "withdrawn" | "cancelled" | "closed_duplicate" | "refused") {
        crate::operations::hooks::on_case_cancelled(tx, state, actor, case_id, reason).await?;
    }
    deadlines::api::on_trigger_at(tx, case_id, "closed", state.now()).await?;
    let case = core::load_case(tx, case_id).await?;
    crate::records::api::on_case_closed(tx, &case).await?;
    record(
        tx,
        actor,
        case_id,
        "case.closed",
        Visibility::Applicant,
        &format!("Request {}: {reason}", outcome.replace('_', " ")),
        json!({"outcome":outcome,"reason":reason}),
    )
    .await?;
    crate::notify::send(
        tx,
        crate::notify::Notice {
            user_id: case.applicant_user_id,
            email: case.applicant_email,
            phone: case.applicant_phone,
            case_id: Some(case_id),
            subject: "Request outcome".into(),
            body: format!("Your request is {}. {reason}", outcome.replace('_', " ")),
            link: Some(format!("/my/cases/{case_id}")),
        },
    )
    .await?;
    core::reindex_search(tx, case_id).await
}
#[derive(Deserialize)]
struct ActionInput {
    expected_revision: i64,
    #[serde(default)]
    reason: String,
    body: Option<String>,
    of_case: Option<i64>,
    document_version_id: Option<i64>,
}
async fn action(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, action)): Path<(i64, String)>,
    Json(input): Json<ActionInput>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&state.db).await?;
    let (case, access) = authz::require_case(&mut tx, &actor, id).await?;
    let def = definition::load_for_case(&mut tx, &case).await?;
    if !allowed_actions(&actor, &case, access, &def).contains(&action.as_str()) {
        return Err(AppError::forbidden());
    }
    if action != "advance"
        && input.reason.trim().is_empty()
        && input.body.as_deref().is_none_or(|s| s.trim().is_empty())
    {
        return Err(AppError::field("reason", "Record a reason for this action."));
    }
    match action.as_str() {
        "advance" => {
            advance(&mut tx, &state, &actor, id, input.expected_revision, Some(input.reason)).await?;
        }
        "skip" => {
            transition(&mut tx, &state, &actor, &case, &def, input.expected_revision, true, &input.reason).await?;
        }
        _ => {
            core::bump_revision(&mut tx, id, Some(input.expected_revision)).await?;
            match action.as_str() {
                "request-info" => {
                    super::messages::post_staff_message_at(
                        &mut tx,
                        &actor,
                        id,
                        input.body.as_deref().unwrap_or(&input.reason),
                        input.document_version_id,
                        true,
                        state.now(),
                    )
                    .await?;
                }
                "reopen" => {
                    let step = def
                        .step(case.current_step.as_deref().unwrap_or(""))
                        .filter(|s| s.kind != StepKind::Complete)
                        .or_else(|| def.workflow.steps.iter().rev().find(|s| s.kind != StepKind::Complete))
                        .ok_or_else(|| AppError::conflict("This workflow has no review step."))?;
                    sqlx::query("UPDATE cases SET status='in_progress',closed_at=NULL,reopened_count=reopened_count+1,current_step=? WHERE id=?").bind(&step.key).bind(id).execute(&mut *tx).await?;
                    record(
                        &mut tx,
                        &actor,
                        id,
                        "case.reopened",
                        Visibility::Applicant,
                        "Request reopened for further review.",
                        json!({"reason":input.reason}),
                    )
                    .await?;
                    let reopened = core::load_case(&mut tx, id).await?;
                    enter_step(&mut tx, &state, &actor, &reopened, step).await?;
                }
                "close-duplicate" => {
                    let target =
                        input.of_case.ok_or_else(|| AppError::field("of_case", "Choose the original request."))?;
                    if target == id {
                        return Err(AppError::field("of_case", "A request cannot be its own duplicate."));
                    }
                    authz::require_staff_case(&mut tx, &actor, target).await?;
                    sqlx::query("INSERT INTO case_links(from_case_id,to_case_id,kind,note,created_by,created_at) VALUES (?,?,'duplicate_of',?,?,?)").bind(id).bind(target).bind(&input.reason).bind(actor.db_id()).bind(time::fmt(state.now())).execute(&mut *tx).await?;
                    close(&mut tx, &state, &actor, id, "closed_duplicate", &input.reason).await?;
                }
                "refuse" | "cancel" | "withdraw" => {
                    close(
                        &mut tx,
                        &state,
                        &actor,
                        id,
                        match action.as_str() {
                            "refuse" => "refused",
                            "cancel" => "cancelled",
                            _ => "withdrawn",
                        },
                        &input.reason,
                    )
                    .await?;
                }
                _ => return Err(AppError::not_found()),
            }
        }
    }
    let updated = core::load_case(&mut tx, id).await?;
    let result = super::search::summary(&mut tx, &updated).await?;
    tx.commit().await?;
    Ok(Json(result))
}
pub async fn projection(tx: &mut SqliteConnection, state: &AppState, actor: &Actor, id: i64) -> AppResult<Value> {
    let (case, access) = authz::require_case(tx, actor, id).await?;
    if matches!(access, CaseAccess::TaskOnly | CaseAccess::None) {
        return Err(AppError::not_found());
    }
    let def = definition::load_for_case(tx, &case).await?;
    let current = def.workflow.steps.iter().position(|s| Some(&s.key) == case.current_step.as_ref());
    let step = current.map(|i| json!({"def":def.workflow.steps[i],"index":i,"total":def.workflow.steps.len()}));
    let steps:Vec<Value>=def.workflow.steps.iter().enumerate().map(|(i,s)|json!({"key":s.key,"label":if access.is_staff(){&s.label}else{&s.applicant_label},"state":if case.status=="completed"||current.is_some_and(|c|i<c){"complete"}else if current==Some(i){"current"}else{"upcoming"}})).collect();
    let required:Option<(i64,String,Option<i64>)>=sqlx::query_as("SELECT id,body,document_version_id FROM case_messages WHERE case_id=? AND requires_response=1 AND resolved_at IS NULL ORDER BY id LIMIT 1").bind(id).fetch_optional(&mut *tx).await?;
    let events:Vec<(i64,String,String,Option<String>)>=sqlx::query_as("SELECT e.id,e.at,e.summary,u.display_name FROM case_events e LEFT JOIN users u ON u.id=e.actor_user_id WHERE e.case_id=? AND (? OR e.visibility='applicant') ORDER BY e.id").bind(id).bind(access.is_staff()).fetch_all(&mut *tx).await?;
    let answers: Option<String> = sqlx::query_scalar("SELECT answers_json FROM submissions WHERE case_id=?")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    let mut result = json!({"case":super::search::summary(tx,&case).await?,"access":access,"step":step,"steps":steps,"allowed_actions":allowed_actions(actor,&case,access,&def),"required_action":required.map(|(id,body,v)|json!({"message_id":id,"body":body,"document_version_id":v})),"definition":def,"answers":answers.map(|a|serde_json::from_str::<Value>(&a)).transpose()?.unwrap_or(json!({})),"timeline":events.into_iter().map(|(id,at,summary,actor)|json!({"id":id,"at":at,"summary":summary,"actor_name":actor})).collect::<Vec<_>>(),"applicant_status_text":super::search::status_text(&case,&def)});
    if access.is_staff() {
        result["guard_reason"] = if let Some(i) = current {
            json!(hooks::step_guard(tx, &case, &def.workflow.steps[i]).await?)
        } else {
            Value::Null
        };
        result["assignments"] = super::assignment::history(tx, id).await?;
        result["case"]["intake_channel"] = json!(case.intake_channel);
        result["case"]["applicant_email"] = json!(case.applicant_email);
        result["case"]["applicant_phone"] = json!(case.applicant_phone);
    }
    let deadlines: Vec<(i64, String, String, String, Option<i64>)> =
        sqlx::query_as("SELECT id,label,due_at,status,max_pause_days FROM deadlines WHERE case_id=?")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    let mut ds = vec![];
    for (id, label, due, status, cap) in deadlines {
        let used = deadlines::api::used_pause_days(tx, id, state.now()).await?;
        ds.push(json!({"id":id,"label":label,"due_at":due,"status":status,"pause_days_used":used,"max_pause_days":cap,"text":if status=="paused" {format!("Your reply pauses the clock: {used} of {} pause days used",cap.unwrap_or(0))}else{format!("We will reply by {}",time::display_local(time::parse(&due)?))}}));
    }
    result["deadlines"] = json!(ds);
    Ok(result)
}
async fn detail(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut tx = state.db.acquire().await?;
    Ok(Json(projection(&mut tx, &state, &actor, id).await?))
}
