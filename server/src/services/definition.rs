// OWNER: services
//! Service definition format (`service_versions.definition_json`, ARCHITECTURE §5).
//! These types deserialize the full format; unknown field types or step kinds fail to deserialize.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::SqliteConnection;

use crate::authz::Role;
use crate::cases::core::CaseRow;
use crate::error::{AppError, AppResult};

/// A whole service definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceDefinition {
    #[serde(default = "default_module")]
    pub module: String,
    pub summary: String,
    pub outcome: String,
    #[serde(default)]
    pub who_can_apply: String,
    #[serde(default)]
    pub price_note: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Optional applicant-facing conditions; older definitions omit this list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<String>,
    #[serde(default)]
    pub fields: Vec<FieldDef>,
    #[serde(default)]
    pub documents: Vec<DocumentRequirement>,
    pub workflow: Workflow,
    #[serde(default)]
    pub deadlines: Vec<DeadlinePolicy>,
    /// Fixed price items for generic services (`[{ "item": "PLANNING_CERT", "quantity": 1 }]`).
    #[serde(default)]
    pub pricing: Vec<PricingItem>,
    /// Building module only: how a submitted case relates to a building project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub building_role: Option<BuildingRole>,
}

/// `project` starts a building project, `modification` modifies an issued approval of one, `follow_up`
/// (commencement, stage and completion notices) is lodged against an existing project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildingRole {
    Project,
    Modification,
    FollowUp,
}

impl ServiceDefinition {
    /// Parses `definition_json`.
    pub fn parse(json: &str) -> AppResult<ServiceDefinition> {
        serde_json::from_str(json).map_err(|e| AppError::validation_msg(format!("Invalid service definition: {e}")))
    }

    /// Step by key.
    pub fn step(&self, key: &str) -> Option<&StepDef> {
        self.workflow.steps.iter().find(|s| s.key == key)
    }

    /// Field by key.
    pub fn field(&self, key: &str) -> Option<&FieldDef> {
        self.fields.iter().find(|f| f.key == key)
    }
}

/// Supported field types. No arbitrary expressions or code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Text,
    Textarea,
    Number,
    Date,
    Time,
    Email,
    Phone,
    Select,
    Multiselect,
    Checkbox,
    /// Portion/Lot reference (string).
    PropertyRef,
    /// `{lat, lng, description}`.
    Location,
    /// Operations: `{unit_code, start_at, end_at, attendees}`.
    BookingSlot,
    /// Operations: `{description, requested_hours, preferred_date, site_text}`.
    EquipmentRequest,
    /// Documents: `{decision_id}`.
    DecisionRef,
    /// Documents: building project reference or ID (string), chosen from the applicant's projects.
    ProjectRef,
}

/// One form field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDef {
    pub key: String,
    #[serde(rename = "type")]
    pub field_type: FieldType,
    pub label: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SelectOption>,
    /// Conditional display; `required` only applies while shown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_if: Option<ShowIf>,
    /// Module-specific properties (e.g. `"venue": "Rawson Hall"` on a `booking_slot`).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

/// `show_if {field, equals}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShowIf {
    pub field: String,
    pub equals: Value,
}

/// A document the applicant uploads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentRequirement {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub accept: Vec<String>,
    #[serde(default)]
    pub public_candidate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workflow {
    pub steps: Vec<StepDef>,
}

/// Workflow step kinds (guards in ARCHITECTURE §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Review,
    Payment,
    Decision,
    Task,
    Module,
    Complete,
}

/// One workflow step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepDef {
    pub key: String,
    pub kind: StepKind,
    /// Staff role that works this step (`None` for `complete`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    pub label: String,
    #[serde(default)]
    pub applicant_label: String,
    /// `module` steps: registered handler, e.g. `operations.booking_confirmed`, `documents.letter_issued:road_response`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handler: Option<String>,
    /// Staff holding `role` may skip it with a recorded reason.
    #[serde(default)]
    pub optional: bool,
    /// `decision` steps: every type must have an issued decision.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decision_types: Vec<String>,
    /// `task` steps: `tasks.kind` to create on entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_kind: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeadlineBasis {
    Business,
    Calendar,
}

/// A statutory/service deadline policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeadlinePolicy {
    pub kind: String,
    pub label: String,
    pub days: i64,
    pub basis: DeadlineBasis,
    /// Trigger: `"submitted"`, `"step:<key>"`, `"decision_issued"`.
    pub starts: String,
    /// Trigger that meets the deadline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stops: Option<String>,
    #[serde(default)]
    pub pausable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_pause_days: Option<i64>,
}

/// Fixed pricing line of a generic service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PricingItem {
    /// `price_items.code`.
    pub item: String,
    #[serde(default = "one")]
    pub quantity: f64,
}

fn one() -> f64 {
    1.0
}

impl PricingItem {
    /// Quantity in thousandths (`invoice_lines.quantity_milli`).
    pub fn quantity_milli(&self) -> i64 {
        (self.quantity * 1000.0).round() as i64
    }
}

/// The frozen definition a case runs on: the submission snapshot if submitted, else the case's
/// service version (drafts).
pub async fn load_for_case(conn: &mut SqliteConnection, case: &CaseRow) -> AppResult<ServiceDefinition> {
    let json: Option<String> = sqlx::query_scalar(
        "SELECT COALESCE((SELECT definition_snapshot_json FROM submissions WHERE case_id = ?), \
                         (SELECT definition_json FROM service_versions WHERE id = ?))",
    )
    .bind(case.id)
    .bind(case.service_version_id)
    .fetch_one(&mut *conn)
    .await?;
    let json = json.ok_or_else(|| AppError::internal(format!("case {} has no definition", case.id)))?;
    ServiceDefinition::parse(&json)
}

fn default_module() -> String {
    "generic".into()
}

pub use super::validation::{ValidationIssue, validate_definition};

#[cfg(test)]
mod tests {
    use super::*;

    /// The example from ARCHITECTURE §5 (comments removed) must deserialize.
    #[test]
    fn architecture_example_deserializes() {
        let json = r#"{
          "summary": "Hire one or both rooms of Rawson Hall for an event.",
          "outcome": "A confirmed booking with date, rooms and conditions (PDF).",
          "who_can_apply": "Anyone; businesses can apply on behalf of their organisation.",
          "price_note": "Session fee per room plus a refundable bond. Illustrative demo prices.",
          "keywords": ["hall", "venue", "party", "wedding", "function", "room hire"],
          "fields": [
            { "key": "event_name", "type": "text", "label": "Event name", "required": true, "max_length": 120 },
            { "key": "slot", "type": "booking_slot", "label": "Date, time and space", "required": true, "venue": "Rawson Hall" },
            { "key": "alcohol", "type": "select", "label": "Will alcohol be served?", "required": true,
              "options": [ { "value": "no", "label": "No" }, { "value": "yes", "label": "Yes" } ] },
            { "key": "liquor_permit", "type": "text", "label": "Liquor permit number", "required": true,
              "show_if": { "field": "alcohol", "equals": "yes" } }
          ],
          "documents": [
            { "key": "insurance", "label": "Public liability insurance (if applicable)", "required": false,
              "accept": ["application/pdf", "image/png", "image/jpeg"], "public_candidate": false }
          ],
          "workflow": { "steps": [
            { "key": "intake", "kind": "review", "role": "intake", "label": "Check request", "applicant_label": "We are checking your request." },
            { "key": "payment", "kind": "payment", "role": "finance", "label": "Fees and bond paid", "applicant_label": "Payment required: hire fee and bond." },
            { "key": "confirm", "kind": "module", "role": "intake", "label": "Confirm booking", "handler": "operations.booking_confirmed",
              "applicant_label": "Payment received — confirming your booking." },
            { "key": "prep", "kind": "task", "role": "field_worker", "task_kind": "venue_prep", "label": "Prepare hall", "applicant_label": "Your booking is confirmed." },
            { "key": "inspect", "kind": "task", "role": "field_worker", "task_kind": "venue_inspection", "label": "Post-event inspection", "applicant_label": "Event finished — hall inspection." },
            { "key": "bond", "kind": "module", "role": "finance", "label": "Bond decision and refund", "handler": "finance.deposits_settled",
              "applicant_label": "We are processing your bond." },
            { "key": "decision", "kind": "decision", "role": "specialist", "decision_types": ["planning_certificate"], "label": "Decide", "optional": true },
            { "key": "done", "kind": "complete", "label": "Closed", "applicant_label": "Hire completed." }
          ]},
          "deadlines": [
            { "kind": "completeness", "label": "Initial check", "days": 3, "basis": "business",
              "starts": "submitted", "stops": "step:payment", "pausable": false }
          ],
          "pricing": [{ "item": "PLANNING_CERT", "quantity": 1 }]
        }"#;
        let d = ServiceDefinition::parse(json).unwrap();
        assert!(d.conditions.is_empty());
        assert_eq!(d.fields[1].field_type, FieldType::BookingSlot);
        assert_eq!(d.fields[1].extra["venue"], "Rawson Hall");
        assert_eq!(d.fields[3].show_if.as_ref().unwrap().field, "alcohol");
        assert_eq!(d.step("confirm").unwrap().handler.as_deref(), Some("operations.booking_confirmed"));
        assert_eq!(d.step("prep").unwrap().role, Some(Role::FieldWorker));
        assert_eq!(d.step("prep").unwrap().task_kind.as_deref(), Some("venue_prep"));
        assert!(d.step("decision").unwrap().optional);
        assert_eq!(d.step("decision").unwrap().decision_types, vec!["planning_certificate"]);
        assert_eq!(d.step("done").unwrap().kind, StepKind::Complete);
        assert_eq!(d.deadlines[0].basis, DeadlineBasis::Business);
        assert_eq!(d.pricing[0].quantity_milli(), 1000);
        // Round-trips.
        let again = ServiceDefinition::parse(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(again, d);
        // Unknown field type is rejected.
        assert!(ServiceDefinition::parse(&json.replace("\"type\": \"text\"", "\"type\": \"script\"")).is_err());
    }
}
