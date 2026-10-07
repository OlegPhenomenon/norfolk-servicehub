//! Structural validation and canonical answer validation shared by publishing and submission.
use super::definition::*;
use crate::{
    error::{AppError, AppResult},
    hooks,
};
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::SqliteConnection;
use std::collections::{BTreeMap, HashSet};

pub const MODULES: &[&str] =
    &["generic", "venue_booking", "equipment_hire", "building", "planning_certificate", "road_issue", "complaint"];
pub fn decision_types(module: &str) -> Vec<&'static str> {
    let mut types = vec!["service_response"];
    match module {
        "building" => types.extend(["development_approval", "building_approval", "modification_approval"]),
        "planning_certificate" => types.push("planning_certificate"),
        _ => {}
    }
    types
}
pub fn handlers(module: &str) -> Vec<&'static str> {
    let mut handlers = vec!["documents.letter_issued:service_response"];
    match module {
        "venue_booking" => handlers.extend(["operations.booking_confirmed", "finance.deposits_settled"]),
        "equipment_hire" => handlers.extend(["operations.equipment_scheduled", "operations.usage_invoiced"]),
        "building" => handlers.push("documents.exhibition_closed"),
        "road_issue" => handlers.push("documents.letter_issued:road_response"),
        "complaint" => handlers.push("documents.letter_issued:complaint_response"),
        _ => {}
    }
    handlers
}
pub fn capabilities(module: &str) -> Value {
    serde_json::json!({"step_kinds":["review","payment","decision","task","module","complete"],"handlers":handlers(module),"decision_types":decision_types(module),"task_kinds":task_kinds(module)})
}
#[derive(Debug, Clone, Serialize)]
pub struct ValidationIssue {
    pub path: String,
    pub message: String,
}

pub async fn validate_definition(
    tx: &mut SqliteConnection,
    def: &ServiceDefinition,
) -> AppResult<Vec<ValidationIssue>> {
    validate_for_module(tx, def, &def.module).await
}
pub async fn validate_for_module(
    tx: &mut SqliteConnection,
    def: &ServiceDefinition,
    module: &str,
) -> AppResult<Vec<ValidationIssue>> {
    let mut issues = vec![];
    let mut issue = |path: String, message: &str| issues.push(ValidationIssue { path, message: message.into() });
    let mut fields = HashSet::new();
    for (i, f) in def.fields.iter().enumerate() {
        let p = format!("fields.{i}");
        if let Some(c) = &f.show_if
            && !fields.contains(&c.field)
        {
            issue(format!("{p}.show_if"), "Choose an earlier field.");
        }
        if f.key.trim().is_empty() || !fields.insert(f.key.clone()) {
            issue(format!("{p}.key"), "Field keys must be non-empty and unique.");
        }
        if f.label.trim().is_empty() {
            issue(format!("{p}.label"), "Enter a field label.");
        }
        if matches!(f.field_type, FieldType::Select | FieldType::Multiselect)
            && !(module == "complaint" && f.key == "staff_member_concerned")
        {
            let mut options = HashSet::new();
            if f.options.is_empty() || f.options.iter().any(|o| o.value.is_empty() || !options.insert(&o.value)) {
                issue(format!("{p}.options"), "Provide unique, non-empty options.");
            }
        }
        if let (Some(min), Some(max)) = (f.min, f.max)
            && min > max
        {
            issue(format!("{p}.min"), "Minimum cannot exceed maximum.");
        }
    }
    let mut documents = HashSet::new();
    for (i, d) in def.documents.iter().enumerate() {
        if d.key.is_empty() || !documents.insert(&d.key) {
            issue(format!("documents.{i}.key"), "Document keys must be non-empty and unique.");
        }
    }
    let steps = &def.workflow.steps;
    if steps.is_empty() {
        issue("workflow.steps".into(), "Provide at least one step.");
    }
    if steps.iter().filter(|s| s.kind == StepKind::Complete).count() != 1
        || steps.last().is_none_or(|s| s.kind != StepKind::Complete)
    {
        issue("workflow.steps".into(), "Exactly one complete step must be last.");
    }
    let mut keys = HashSet::new();
    for (i, s) in steps.iter().enumerate() {
        let p = format!("workflow.steps.{i}");
        if s.key.is_empty() || !keys.insert(s.key.as_str()) {
            issue(format!("{p}.key"), "Step keys must be non-empty and unique.");
        }
        if s.kind != StepKind::Complete && s.role.is_none() {
            issue(format!("{p}.role"), "Choose the role responsible for this step.");
        }
        if s.kind == StepKind::Complete && s.optional {
            issue(format!("{p}.optional"), "The terminal step cannot be optional.");
        }
        if s.kind == StepKind::Task && !s.task_kind.as_deref().is_some_and(|k| task_kinds(module).contains(&k)) {
            issue(format!("{p}.task_kind"), "Choose a registered task kind.");
        }
        if s.kind == StepKind::Module && !s.handler.as_deref().is_some_and(|h| handlers(module).contains(&h)) {
            issue(format!("{p}.handler"), "Choose a registered module handler.");
        }
        if s.kind == StepKind::Decision
            && (s.decision_types.is_empty()
                || s.decision_types.iter().any(|d| !decision_types(module).contains(&d.as_str())))
        {
            issue(format!("{p}.decision_types"), "Choose registered decision types.");
        }
    }
    let trigger_valid = |t: &str| {
        matches!(t, "submitted" | "decision_issued" | "closed")
            || t.strip_prefix("step:").is_some_and(|k| keys.contains(k))
    };
    let mut kinds = HashSet::new();
    for (i, d) in def.deadlines.iter().enumerate() {
        let p = format!("deadlines.{i}");
        if d.days < 1 || d.days > 3650 {
            issue(format!("{p}.days"), "Enter 1 to 3650 days.");
        }
        if d.kind.is_empty() || !kinds.insert(&d.kind) {
            issue(format!("{p}.kind"), "Deadline kinds must be unique.");
        }
        if !trigger_valid(&d.starts) {
            issue(format!("{p}.starts"), "Choose an existing step or trigger.");
        }
        if d.stops.as_deref().is_some_and(|s| !trigger_valid(s)) {
            issue(format!("{p}.stops"), "Choose an existing step or trigger.");
        }
        if d.pausable && d.max_pause_days.is_none_or(|n| !(1..=3650).contains(&n)) {
            issue(format!("{p}.max_pause_days"), "A pausable deadline needs a positive cumulative pause cap.");
        }
    }
    let required: &[&str] = match module {
        "venue_booking" => &["intake", "payment", "confirm", "prep", "inspect", "bond", "done"],
        "equipment_hire" => &["intake", "schedule", "job", "usage", "payment", "done"],
        "planning_certificate" => &["intake", "payment", "preparation", "decision", "done"],
        "road_issue" => &["triage", "inspection", "repair", "response", "done"],
        "complaint" => &["triage", "investigation", "response", "done"],
        "building"
            if def.field("project_reference").is_some()
                && (def.field("commencement_date").is_some() || def.field("completion").is_some()) =>
        {
            &["intake", "site", "done"]
        }
        "building" => &["intake", "assessment", "decision", "done"],
        _ => &[],
    };
    if !MODULES.contains(&module) {
        issue("module".into(), "Choose a registered service module.");
    }
    for key in required {
        if !keys.contains(key) {
            issue("workflow.steps".into(), &format!("This module requires the {key} step."));
        }
    }
    // A required module checkpoint cannot be relabelled as a review to bypass its guard.
    let checkpoints: &[(&str, StepKind, Option<&str>)] = match module {
        "venue_booking" => &[
            ("payment", StepKind::Payment, None),
            ("confirm", StepKind::Module, Some("operations.booking_confirmed")),
            ("prep", StepKind::Task, None),
            ("inspect", StepKind::Task, None),
            ("bond", StepKind::Module, Some("finance.deposits_settled")),
        ],
        "equipment_hire" => &[
            ("schedule", StepKind::Module, Some("operations.equipment_scheduled")),
            ("job", StepKind::Task, None),
            ("usage", StepKind::Module, Some("operations.usage_invoiced")),
            ("payment", StepKind::Payment, None),
        ],
        "planning_certificate" => &[("payment", StepKind::Payment, None), ("decision", StepKind::Decision, None)],
        "road_issue" => &[
            ("inspection", StepKind::Task, None),
            ("repair", StepKind::Task, None),
            ("response", StepKind::Module, Some("documents.letter_issued:road_response")),
        ],
        "complaint" => &[("response", StepKind::Module, Some("documents.letter_issued:complaint_response"))],
        _ => &[],
    };
    for (key, kind, handler) in checkpoints {
        if let Some(step) = def.step(key)
            && (step.kind != *kind || handler.is_some_and(|h| step.handler.as_deref() != Some(h)))
        {
            issue(
                "workflow.steps".into(),
                &format!("The {key} checkpoint must retain its registered kind and handler."),
            );
        }
    }
    for (i, p) in def.pricing.iter().enumerate() {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM price_items WHERE code = ?)")
            .bind(&p.item)
            .fetch_one(&mut *tx)
            .await?;
        if !exists {
            issues.push(ValidationIssue {
                path: format!("pricing.{i}.item"),
                message: "Unknown price item code.".into(),
            });
        }
        if !p.quantity.is_finite() || p.quantity <= 0.0 {
            issues.push(ValidationIssue {
                path: format!("pricing.{i}.quantity"),
                message: "Quantity must be positive.".into(),
            });
        }
    }
    Ok(issues)
}

pub fn visible(field: &FieldDef, answers: &Map<String, Value>) -> bool {
    field.show_if.as_ref().is_none_or(|s| answers.get(&s.field) == Some(&s.equals))
}
/// Returns only known, visible answers; unknown and hidden input never enters the immutable snapshot.
pub async fn validate_answers(
    tx: &mut SqliteConnection,
    module: &str,
    def: &ServiceDefinition,
    input: &Value,
) -> AppResult<Value> {
    let input = input.as_object().ok_or_else(|| AppError::field("answers", "Answers must be an object."))?;
    let mut cleaned = Map::new();
    let mut errors = BTreeMap::new();
    for f in &def.fields {
        if !visible(f, &cleaned) {
            continue;
        }
        let v = input.get(&f.key).unwrap_or(&Value::Null);
        let empty =
            v.is_null() || v.as_str().is_some_and(|s| s.trim().is_empty()) || v.as_array().is_some_and(Vec::is_empty);
        let error = if empty {
            f.required.then_some("This field is required.".to_owned())
        } else if f.required && f.field_type == FieldType::Checkbox && v != &Value::Bool(true) {
            Some("You must agree to this declaration.".into())
        } else {
            match if module == "complaint" && f.key == "staff_member_concerned" { None } else { value_error(f, v) } {
                Some(message) => Some(message),
                None => hooks::validate_field(tx, module, f, v).await?,
            }
        };
        if let Some(message) = error {
            errors.insert(f.key.clone(), message);
        }
        if !empty {
            cleaned.insert(f.key.clone(), v.clone());
        }
    }
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    Ok(Value::Object(cleaned))
}
fn value_error(f: &FieldDef, v: &Value) -> Option<String> {
    let valid = match f.field_type {
        FieldType::Number => {
            v.as_f64().is_some_and(|n| n.is_finite() && f.min.is_none_or(|m| n >= m) && f.max.is_none_or(|m| n <= m))
        }
        FieldType::Checkbox => v.is_boolean(),
        FieldType::Select => v.as_str().is_some_and(|s| f.options.iter().any(|o| o.value == s)),
        FieldType::Multiselect => v
            .as_array()
            .is_some_and(|a| a.iter().all(|v| v.as_str().is_some_and(|s| f.options.iter().any(|o| o.value == s)))),
        FieldType::Date => {
            v.as_str().is_some_and(|s| s.len() == 10 && chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok())
        }
        FieldType::Time => {
            v.as_str().is_some_and(|s| s.len() == 5 && chrono::NaiveTime::parse_from_str(s, "%H:%M").is_ok())
        }
        FieldType::Email => v.as_str().is_some_and(|s| {
            s.split_once('@').is_some_and(|(a, b)| !a.is_empty() && b.contains('.') && !s.contains(char::is_whitespace))
        }),
        FieldType::Phone => v.as_str().is_some_and(|s| s.chars().filter(char::is_ascii_digit).count() >= 5),
        FieldType::Location => {
            v["lat"].as_f64().is_some_and(|n| (-90.0..=90.0).contains(&n))
                && v["lng"].as_f64().is_some_and(|n| (-180.0..=180.0).contains(&n))
                && v["description"].is_string()
        }
        FieldType::BookingSlot => {
            v["unit_code"].as_str().is_some_and(|s| !s.trim().is_empty())
                && v["start_at"]
                    .as_str()
                    .and_then(|s| crate::time::parse(s).ok())
                    .zip(v["end_at"].as_str().and_then(|s| crate::time::parse(s).ok()))
                    .is_some_and(|(start, end)| end > start)
                && v["attendees"].as_i64().is_some_and(|n| n > 0)
        }
        FieldType::EquipmentRequest => {
            v["description"].as_str().is_some_and(|s| !s.trim().is_empty())
                && v["site_text"].as_str().is_some_and(|s| !s.trim().is_empty())
                && v["preferred_date"].as_str().is_some_and(|s| crate::time::parse_date(s).is_ok())
                && v["requested_hours"].as_f64().is_some_and(|n| n > 0.0 && n.is_finite())
        }
        FieldType::DecisionRef => v["decision_id"].as_i64().is_some_and(|n| n > 0),
        _ => v.as_str().is_some_and(|s| f.max_length.is_none_or(|n| s.chars().count() <= n as usize)),
    };
    (!valid).then(|| "Enter a valid value for this field.".into())
}

pub fn task_kinds(module: &str) -> Vec<&'static str> {
    let mut kinds = vec!["general"];
    match module {
        "venue_booking" => kinds.extend(["venue_prep", "venue_inspection"]),
        "equipment_hire" => kinds.push("equipment_job"),
        "building" => kinds.push("site_inspection"),
        "road_issue" => kinds.extend(["road_inspection", "road_repair"]),
        _ => {}
    }
    kinds
}
