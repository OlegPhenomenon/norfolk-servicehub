//! Timeline wording per audience. Both sides see the same applicant-visible events, so an event whose wording
//! depends on the reader is rendered here from its `kind` and `data`: staff read third person and staff step
//! names; the applicant reads second person and the applicant step labels. The stored `summary` (staff
//! wording) is used for every other event and as the fallback for events without the data.
use crate::services::definition::ServiceDefinition;
use serde_json::Value;

/// How Council received something from the applicant on an assisted request, as shown in the timeline.
pub fn received_how(channel: &str) -> Option<&'static str> {
    match channel {
        "phone" => Some("by phone"),
        "post" => Some("by post"),
        "email" => Some("by email"),
        "walk_in" => Some("in person"),
        _ => None,
    }
}

fn sentence(text: &str) -> String {
    let text = text.trim();
    if text.ends_with(['.', '!', '?']) { text.to_string() } else { format!("{text}.") }
}

/// The summary one audience reads for a timeline event.
pub fn event_summary(def: &ServiceDefinition, kind: &str, data: &Value, stored: &str, staff: bool) -> String {
    let text = |key: &str| data.get(key).and_then(Value::as_str);
    match kind {
        "step.changed" | "step.skipped" => {
            let (Some(from), Some(to)) = (text("from").and_then(|k| def.step(k)), text("to").and_then(|k| def.step(k)))
            else {
                return if staff { stored.to_string() } else { "Your request moved to the next stage.".into() };
            };
            if staff {
                let done = if kind == "step.skipped" { "skipped" } else { "completed" };
                format!("{} {done}. Next: {}.", from.label, to.label)
            } else if to.applicant_label.trim().is_empty() {
                "Your request moved to the next stage.".into()
            } else {
                sentence(&to.applicant_label)
            }
        }
        "message.staff" => match (data.get("requires_response").and_then(Value::as_bool).unwrap_or(false), staff) {
            (true, true) => "Council requested more information from the applicant.".into(),
            (true, false) => "We need more information from you.".into(),
            (false, true) => "Council sent the applicant a message.".into(),
            (false, false) => "Council sent you a message.".into(),
        },
        "message.applicant" => {
            if staff {
                "Applicant replied to Council.".into()
            } else {
                "You replied to Council.".into()
            }
        }
        "message.applicant_recorded" => {
            let how = text("channel").and_then(received_how).unwrap_or("offline");
            if staff {
                let by = text("recorded_by_name").unwrap_or("Council staff");
                format!("Applicant's reply received {how}. Recorded by {by} on the applicant's behalf.")
            } else {
                format!("Your reply received {how} was recorded by Council on your behalf.")
            }
        }
        "case.withdrawn_on_behalf" => {
            let how = text("channel").and_then(received_how).unwrap_or("offline");
            if staff {
                let by = text("recorded_by_name").unwrap_or("Council staff");
                format!("Applicant asked {how} to withdraw the request. Recorded by {by} on the applicant's behalf.")
            } else {
                format!("Your request to withdraw, received {how}, was recorded by Council on your behalf.")
            }
        }
        "documents.version_on_behalf" => {
            let how = text("channel").and_then(received_how).unwrap_or("offline");
            let title = text("title").unwrap_or("a document");
            let version =
                data.get("version").and_then(Value::as_i64).map(|v| format!("version {v} of ")).unwrap_or_default();
            if staff {
                let by = text("recorded_by_name").unwrap_or("Council staff");
                format!("Applicant's {version}{title} received {how}. Attached by {by} on the applicant's behalf.")
            } else {
                format!("Your {version}{title}, received {how}, was attached by Council on your behalf.")
            }
        }
        "draft.version_updated" => {
            if staff {
                "The form was updated to the current version; the answers need review.".into()
            } else {
                "The form was updated to the current version; please review your answers.".into()
            }
        }
        "representative.added" => {
            if staff {
                "Applicant authorised a representative.".into()
            } else {
                "A representative was authorised to act on this request.".into()
            }
        }
        // Finance corrections (`finance::ledger::event_with_note`): staff read the internal note.
        _ if staff && text("staff_note").is_some() => text("staff_note").unwrap_or(stored).to_string(),
        _ => stored.to_string(),
    }
}
