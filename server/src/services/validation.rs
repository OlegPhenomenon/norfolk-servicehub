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
        "building" => handlers.extend(["documents.exhibition_closed", "finance.fee_assessed"]),
        "road_issue" => handlers.push("documents.letter_issued:road_response"),
        "complaint" => handlers.push("documents.letter_issued:complaint_response"),
        _ => {}
    }
    handlers
}
/// What the Builder may offer for `module`. Every option listed here is executable end to end: the server has a
/// guard/handler for it and the staff UI exposes the action that satisfies it (`tests/audit2_builder_forms.rs`).
pub fn capabilities(module: &str) -> Value {
    let mut step_kinds = vec!["review", "payment", "decision"];
    if !task_kinds(module).is_empty() {
        step_kinds.push("task");
    }
    step_kinds.extend(["module", "complete"]);
    serde_json::json!({"step_kinds":step_kinds,"handlers":handlers(module),"decision_types":decision_types(module),"task_kinds":task_kinds(module)})
}
/// Column types of a `group` field (contract: simple scalar inputs only).
fn group_column_type(t: FieldType) -> bool {
    matches!(
        t,
        FieldType::Text
            | FieldType::Textarea
            | FieldType::Number
            | FieldType::Date
            | FieldType::Email
            | FieldType::Phone
            | FieldType::Select
            | FieldType::Checkbox
    )
}
/// Label, option and range checks shared by top-level fields and group columns.
fn field_shape_issues(p: &str, f: &FieldDef, module: &str) -> Vec<(String, &'static str)> {
    let mut out = vec![];
    if f.label.trim().is_empty() {
        out.push((format!("{p}.label"), "Enter a field label."));
    }
    if matches!(f.field_type, FieldType::Select | FieldType::Multiselect)
        && !(module == "complaint" && f.key == "staff_member_concerned")
    {
        let mut options = HashSet::new();
        if f.options.is_empty() || f.options.iter().any(|o| o.value.is_empty() || !options.insert(&o.value)) {
            out.push((format!("{p}.options"), "Provide unique, non-empty options."));
        }
    }
    if let (Some(min), Some(max)) = (f.min, f.max)
        && min > max
    {
        out.push((format!("{p}.min"), "Minimum cannot exceed maximum."));
    }
    out
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
        for (path, message) in field_shape_issues(&p, f, module) {
            issue(path, message);
        }
        if f.field_type == FieldType::Group {
            if f.columns.is_empty() {
                issue(format!("{p}.columns"), "Add at least one column to a repeating group.");
            }
            if let (Some(min), Some(max)) = (f.min_items, f.max_items)
                && min > max
            {
                issue(format!("{p}.min_items"), "Minimum rows cannot exceed maximum rows.");
            }
            if f.max_items == Some(0) {
                issue(format!("{p}.max_items"), "Allow at least one row.");
            }
            let mut columns = HashSet::new();
            for (j, c) in f.columns.iter().enumerate() {
                let cp = format!("{p}.columns.{j}");
                if c.key.trim().is_empty() || !columns.insert(c.key.as_str()) {
                    issue(format!("{cp}.key"), "Column keys must be non-empty and unique.");
                }
                if !group_column_type(c.field_type) {
                    issue(
                        format!("{cp}.type"),
                        "Use a text, long text, number, date, email, phone, select or checkbox column.",
                    );
                }
                if c.show_if.is_some() || !c.columns.is_empty() {
                    issue(cp.clone(), "Columns cannot be conditional or nested.");
                }
                for (path, message) in field_shape_issues(&cp, c, module) {
                    issue(path, message);
                }
            }
        } else if !f.columns.is_empty() || f.min_items.is_some() || f.max_items.is_some() {
            issue(format!("{p}.columns"), "Only repeating groups have columns and row limits.");
        }
    }
    let mut documents = HashSet::new();
    for (i, d) in def.documents.iter().enumerate() {
        if d.key.is_empty() || !documents.insert(&d.key) {
            issue(format!("documents.{i}.key"), "Document keys must be non-empty and unique.");
        }
        if let Some(c) = &d.show_if
            && !fields.contains(&c.field)
        {
            issue(format!("documents.{i}.show_if"), "Choose a field from this form.");
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
        // A modification decision supersedes the approval chosen in the request; without that field it can never
        // be prepared ("Link the request to its original approval first").
        if s.decision_types.iter().any(|d| d == "modification_approval")
            && def.field("original_approval").is_none_or(|f| f.field_type != FieldType::DecisionRef)
        {
            issue(
                format!("{p}.decision_types"),
                "A modification approval needs an 'original_approval' issued-approval field in the form.",
            );
        }
        // A building project issues only its DA/BA, a modification only modification approvals and a follow-up
        // notice only the permission to continue; any other type could never be prepared for the role. Without a
        // role nothing links the request to the approvals it modifies, so a modification approval is unusable.
        if module == "building"
            && s.decision_types.iter().any(|d| !crate::documents::building::role_permits(def.building_role, d))
        {
            issue(
                format!("{p}.decision_types"),
                if def.building_role.is_none() {
                    "A modification approval needs the building role 'modification'."
                } else {
                    "This decision type does not match the service's building role."
                },
            );
        }
        // The fee assessment is the building approval route's own checkpoint: only projects and modifications
        // have the fee rules, and the assessment is invoiced by a later payment step (an earlier payment step would
        // try to invoice before any assessment exists).
        if s.handler.as_deref() == Some(crate::finance::building_fees::HANDLER) {
            if !matches!(def.building_role, Some(BuildingRole::Project | BuildingRole::Modification)) {
                issue(
                    format!("{p}.handler"),
                    "Fee assessment is only available to a building project or modification service.",
                );
            }
            if !steps[i + 1..].iter().any(|t| t.kind == StepKind::Payment)
                || steps[..i].iter().any(|t| t.kind == StepKind::Payment)
            {
                issue(format!("{p}.handler"), "Fee assessment must come before the payment step that invoices it.");
            }
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
        "building" => match def.building_role {
            Some(BuildingRole::FollowUp) => &["intake", "site", "done"],
            Some(BuildingRole::Project | BuildingRole::Modification) => {
                &["intake", "fees", "payment", "assessment", "exhibition", "decision", "done"]
            }
            None => &["intake", "assessment", "decision", "done"],
        },
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
    if def.building_role.is_some() && module != "building" {
        issue("building_role".into(), "Only building services can relate to a building project.");
    }
    if module == "building" && def.building_role == Some(BuildingRole::FollowUp) {
        if !def.fields.iter().any(|f| f.field_type == FieldType::ProjectRef && f.required && f.show_if.is_none()) {
            issue(
                "fields".into(),
                "A follow-up building service needs a required building project field (project_ref).",
            );
        }
        if def.step("site").is_some_and(|s| s.kind != StepKind::Task) {
            issue("workflow.steps".into(), "The site checkpoint must remain an inspection task.");
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
        "building" if matches!(def.building_role, Some(BuildingRole::Project | BuildingRole::Modification)) => &[
            ("fees", StepKind::Module, Some(crate::finance::building_fees::HANDLER)),
            ("payment", StepKind::Payment, None),
            ("exhibition", StepKind::Module, Some("documents.exhibition_closed")),
            ("decision", StepKind::Decision, None),
        ],
        _ => &[],
    };
    let approval_route = module == "building" && !checkpoints.is_empty();
    for (key, kind, handler) in checkpoints {
        if let Some(step) = def.step(key)
            && (step.kind != *kind || handler.is_some_and(|h| step.handler.as_deref() != Some(h)))
        {
            issue(
                "workflow.steps".into(),
                &format!("The {key} checkpoint must retain its registered kind and handler."),
            );
        }
        // Fee, payment, public exhibition and decision gates of a building approval route cannot be skipped.
        if approval_route && def.step(key).is_some_and(|s| s.optional) {
            issue("workflow.steps".into(), &format!("The {key} checkpoint of a building approval cannot be optional."));
        }
    }
    // Presence is not completion: a building approval is issued only after its fee is invoiced and paid, the
    // assessment is done and the public exhibition is settled, so every decision step comes after those checkpoints.
    // One issue per decision step (the publish error is keyed by path), naming the lowest checkpoint it must follow.
    if approval_route {
        for (i, s) in steps.iter().enumerate().filter(|(_, s)| s.kind == StepKind::Decision) {
            if let Some(gate) = steps[i + 1..]
                .iter()
                .rev()
                .find(|t| ["fees", "payment", "assessment", "exhibition"].contains(&t.key.as_str()))
            {
                issue(
                    format!("workflow.steps.{i}"),
                    &format!(
                        "Move the {} step below the {} step: a building approval is issued only after the fee assessment, payment, assessment and public exhibition are settled.",
                        s.key, gate.key
                    ),
                );
            }
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
    field.show_if.as_ref().is_none_or(|s| s.matches(answers.get(&s.field)))
}
fn blank(v: &Value) -> bool {
    v.is_null() || v.as_str().is_some_and(|s| s.trim().is_empty()) || v.as_array().is_some_and(Vec::is_empty)
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
        let v = &canonical(f, v);
        // A group whose rows are all empty (the form's "Add row" starts with one) has no answer.
        let empty = blank(v)
            || v.as_array()
                .is_some_and(|rows| f.field_type == FieldType::Group && !rows.iter().any(|r| filled_row(f, r)));
        if f.field_type == FieldType::Group {
            if empty {
                if f.required {
                    errors.insert(f.key.clone(), "Add at least one row.".into());
                }
            } else if let Some(rows) = group_answer(f, v, &mut errors) {
                cleaned.insert(f.key.clone(), rows);
            }
            continue;
        }
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
/// Canonical answer shape before validation: a multiselect keeps each value once (in first-chosen order), so a
/// repeated choice never changes rules such as "lapse date only".
fn canonical(f: &FieldDef, v: &Value) -> Value {
    match (f.field_type, v.as_array()) {
        (FieldType::Multiselect, Some(values)) => {
            let mut unique: Vec<Value> = Vec::with_capacity(values.len());
            for value in values {
                if !unique.contains(value) {
                    unique.push(value.clone());
                }
            }
            Value::Array(unique)
        }
        _ => v.clone(),
    }
}
/// A group row with at least one filled-in cell (an unticked checkbox is not filled in). Rows that are not objects
/// count as filled so they are reported as invalid.
fn filled_row(f: &FieldDef, row: &Value) -> bool {
    row.as_object().is_none_or(|cells| {
        f.columns.iter().any(|c| cells.get(&c.key).is_some_and(|cell| !blank(cell) && cell != &Value::Bool(false)))
    })
}
/// Validates a `group` answer row by row and returns the rows reduced to known, non-empty columns. Row problems
/// are reported on the field and on `<field>.<row>.<column>` so the form can mark the exact cell.
fn group_answer(f: &FieldDef, v: &Value, errors: &mut BTreeMap<String, String>) -> Option<Value> {
    let Some(rows) = v.as_array() else {
        errors.insert(f.key.clone(), "Enter a valid value for this field.".into());
        return None;
    };
    // Empty rows are ignored (not counted, not validated, not stored); errors keep the row's position in the form.
    let count = u32::try_from(rows.iter().filter(|r| filled_row(f, r)).count()).unwrap_or(u32::MAX);
    let mut field_error = None;
    if let Some(min) = f.min_items.filter(|m| count < *m) {
        field_error = Some(format!("Add at least {min} rows."));
    }
    if let Some(max) = f.max_items.filter(|m| count > *m) {
        field_error = Some(format!("Enter no more than {max} rows."));
    }
    let mut cell_errors = false;
    let mut cleaned = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate().filter(|(_, r)| filled_row(f, r)) {
        let Some(row) = row.as_object() else {
            errors.insert(format!("{}.{i}", f.key), "Enter a valid row.".into());
            cell_errors = true;
            continue;
        };
        let mut clean = Map::new();
        for c in &f.columns {
            let cell = row.get(&c.key).unwrap_or(&Value::Null);
            let empty = blank(cell);
            let error = if empty {
                c.required.then(|| "This field is required.".to_owned())
            } else if c.required && c.field_type == FieldType::Checkbox && cell != &Value::Bool(true) {
                Some("Confirm this item.".into())
            } else {
                value_error(c, cell)
            };
            if let Some(message) = error {
                errors.insert(format!("{}.{i}.{}", f.key, c.key), message);
                cell_errors = true;
            }
            if !empty {
                clean.insert(c.key.clone(), cell.clone());
            }
        }
        cleaned.push(Value::Object(clean));
    }
    if field_error.is_none() && cell_errors {
        field_error = Some("Check the highlighted rows.".into());
    }
    match field_error {
        Some(message) => {
            errors.insert(f.key.clone(), message);
            None
        }
        None => Some(Value::Array(cleaned)),
    }
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
        FieldType::DecisionRef => {
            v["decision_id"].as_i64().is_some_and(|n| n > 0)
                || v["decision_ids"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty() && a.iter().all(|n| n.as_i64().is_some_and(|n| n > 0)))
        }
        _ => v.as_str().is_some_and(|s| f.max_length.is_none_or(|n| s.chars().count() <= n as usize)),
    };
    (!valid).then(|| "Enter a valid value for this field.".into())
}

/// Task kinds a step may create. Complaint cases are always confidential and field workers can never be given
/// access to them (`authz::case_access`, `operations::tasks::eligible_worker`), so a field task there could never
/// be finished; the Builder therefore offers no task steps for complaints.
pub fn task_kinds(module: &str) -> Vec<&'static str> {
    let mut kinds = vec!["general"];
    match module {
        "venue_booking" => kinds.extend(["venue_prep", "venue_inspection"]),
        "equipment_hire" => kinds.push("equipment_job"),
        "building" => kinds.push("site_inspection"),
        "road_issue" => kinds.extend(["road_inspection", "road_repair"]),
        "complaint" => kinds.clear(),
        _ => {}
    }
    kinds
}
