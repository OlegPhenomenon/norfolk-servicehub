//! Cross-module task API; instructions are an explicit worker-only projection.
use super::{model, tasks};
use crate::{
    cases::core::CaseRow,
    error::{AppError, AppResult},
    services::definition::StepDef,
    time,
};
use chrono::Duration;
use serde_json::{Value, json};
use sqlx::SqliteConnection;

pub async fn create_step_task(
    tx: &mut SqliteConnection,
    case: &CaseRow,
    step: &StepDef,
    step_run_id: i64,
    actor: Option<i64>,
) -> AppResult<i64> {
    let belongs: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_step_runs WHERE id=? AND case_id=?)")
        .bind(step_run_id)
        .bind(case.id)
        .fetch_one(&mut *tx)
        .await?;
    if !belongs {
        return Err(AppError::internal("Task step run belongs to a different case"));
    }
    if let Some(id) = sqlx::query_scalar::<_, i64>("SELECT id FROM tasks WHERE step_run_id=?")
        .bind(step_run_id)
        .fetch_optional(&mut *tx)
        .await?
    {
        return Ok(id);
    }
    let kind = step.task_kind.as_deref().unwrap_or("general");
    let (title, instructions, start, end, location, checklist, operator) = task_spec(tx, case, kind).await?;
    let default:Option<i64>=sqlx::query_scalar("SELECT u.id FROM users u JOIN role_grants g ON g.user_id=u.id WHERE NOT EXISTS(SELECT 1 FROM case_access_denials d WHERE d.case_id=? AND d.user_id=u.id) AND u.is_active=1 AND g.role='field_worker' AND g.revoked_at IS NULL AND (g.scope_service_id IS NULL OR g.scope_service_id=?) ORDER BY CASE WHEN u.persona_key='jake' THEN 0 ELSE 1 END,u.id LIMIT 1").bind(case.id).bind(case.service_id).fetch_optional(&mut *tx).await?;
    let assignee = if case.is_confidential() { None } else { operator.or(default) };
    let id:i64=sqlx::query_scalar("INSERT INTO tasks(case_id,kind,title,instructions,assigned_to,scheduled_start,scheduled_end,location_text,location_lat,location_lng,checklist_json,status,created_by,created_at,step_run_id) VALUES (?,?,?,?,?,?,?,?,?,?,?,'open',?,?,?) RETURNING id")
        .bind(case.id).bind(kind).bind(&title).bind(instructions).bind(assignee).bind(start).bind(end).bind(location).bind(case.location_lat).bind(case.location_lng).bind(checklist.to_string()).bind(actor).bind(time::now_str()).bind(step_run_id).fetch_one(&mut *tx).await?;
    if kind == "equipment_job" {
        sqlx::query("UPDATE equipment_requests SET task_id=? WHERE case_id=?")
            .bind(id)
            .bind(case.id)
            .execute(&mut *tx)
            .await?;
    }
    model::record(
        tx,
        actor,
        case.id,
        "task.created",
        &format!("Field task created: {title}."),
        json!({"task_id":id,"step_run_id":step_run_id}),
    )
    .await?;
    tasks::notify_assignee(tx, id, assignee, &title).await?;
    Ok(id)
}
// Keep this tuple private: it is built from whitelisted operational fields only.
type TaskSpec = (String, String, Option<String>, Option<String>, Option<String>, Value, Option<i64>);
pub(super) async fn task_spec(tx: &mut SqliteConnection, c: &CaseRow, kind: &str) -> AppResult<TaskSpec> {
    let a = model::answers(tx, c.id).await?;
    let (title, instructions, start, end, location, labels, operator) = match kind {
        "venue_prep" | "venue_inspection" => {
            let b = model::booking(tx, c.id).await?;
            let u: model::Unit =
                sqlx::query_as("SELECT * FROM bookable_units WHERE id=?").bind(b.unit_id).fetch_one(&mut *tx).await?;
            let resources = model::unit_resources(tx, u.id).await?;
            let prep = resources.iter().map(|r| r.prep_minutes).max().unwrap_or(60);
            let cleanup = resources.iter().map(|r| r.cleanup_minutes).max().unwrap_or(60);
            let event = a["event_name"].as_str().or_else(|| a["event_title"].as_str()).unwrap_or("Council venue hire");
            let setup = a["setup_notes"]
                .as_str()
                .or_else(|| a["other_instructions"].as_str())
                .unwrap_or("No special setup requested.");
            let verb = if kind == "venue_prep" { "Prepare" } else { "Inspect" };
            let title = format!("{verb} {} for '{event}'", u.name);
            let instructions = format!(
                "Event: {event}\nSpace: {}\nHire: {}–{}\nGuests: {}\nSetup notes: {setup}\n{}",
                u.name,
                time::display_local(time::parse(&b.start_at)?),
                time::display_local(time::parse(&b.end_at)?),
                b.attendees.unwrap_or(0),
                if kind == "venue_prep" {
                    "Council staff move furniture. Check approved decorations and safety exits."
                } else {
                    "Record damage, cleaning, rubbish and key return. Finance decides any bond retention."
                }
            );
            let (start, end) = if kind == "venue_prep" {
                (time::fmt(time::parse(&b.start_at)? - Duration::minutes(prep)), b.start_at)
            } else {
                (b.end_at.clone(), time::fmt(time::parse(&b.end_at)? + Duration::minutes(cleanup)))
            };
            (
                title,
                instructions,
                Some(start),
                Some(end),
                Some("Rawson Hall, Taylors Road, Burnt Pine".into()),
                if kind == "venue_prep" {
                    vec!["Room prepared", "Safety exits clear"]
                } else {
                    vec!["Cleaning and damage recorded", "Key return checked"]
                },
                None,
            )
        }
        "equipment_job" => {
            let e: super::equipment::Request = sqlx::query_as("SELECT * FROM equipment_requests WHERE case_id=?")
                .bind(c.id)
                .fetch_one(&mut *tx)
                .await?;
            (
                "Council plant hire job".into(),
                format!(
                    "Work: {}\nSite: {}\nRequested hours: {}\nRecord depot departure/return times, downtime and agreed expenses. Report accidents and damage immediately.",
                    e.description,
                    e.site_text.as_deref().unwrap_or("Confirm site"),
                    e.requested_hours.unwrap_or(0)
                ),
                e.scheduled_start,
                e.scheduled_end,
                e.site_text,
                vec!["Safety and site checked", "Job card recorded"],
                e.operator_user_id,
            )
        }
        "road_inspection" | "road_repair" => (
            if kind == "road_inspection" { "Inspect reported road issue" } else { "Repair road issue" }.into(),
            format!(
                "Location: {}\nInspect the reported defect and record the work/result. Use traffic safety controls.",
                c.location_text.as_deref().unwrap_or("See map pin")
            ),
            None,
            None,
            c.location_text.clone(),
            vec!["Site safety checked", "Findings or repair recorded"],
            None,
        ),
        "site_inspection" => (
            "Inspect building site".into(),
            format!(
                "Property: {}. Record site findings and required follow-up.",
                c.property_ref.as_deref().unwrap_or("Confirm property with Planning")
            ),
            None,
            None,
            c.property_ref.clone(),
            vec!["Site inspected", "Findings recorded"],
            None,
        ),
        "general" => (
            "Council field task".into(),
            "Perform the assigned site check and record the result.".into(),
            None,
            None,
            c.location_text.clone(),
            vec!["Work recorded"],
            None,
        ),
        _ => return Err(AppError::internal(format!("Unknown task kind {kind}"))),
    };
    let checklist = Value::Array(
        labels
            .iter()
            .enumerate()
            .map(|(i, label)| json!({"key":format!("check-{i}"),"label":label,"done":false}))
            .collect(),
    );
    Ok((title, instructions, start, end, location, checklist, operator))
}
pub async fn step_tasks_done(tx: &mut SqliteConnection, case: i64, run: i64) -> AppResult<bool> {
    let (count,unfinished):(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(sum(CASE WHEN status<>'done' THEN 1 ELSE 0 END),0) FROM tasks WHERE case_id=? AND step_run_id=?").bind(case).bind(run).fetch_one(&mut *tx).await?;
    Ok(count > 0 && unfinished == 0)
}
