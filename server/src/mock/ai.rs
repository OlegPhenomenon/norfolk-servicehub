//! Deterministic local mock: labelled blanks and attachment checklists, never financial rules.
use crate::{
    error::{AppError, AppResult},
    state::AppState,
    web::Json,
};
use axum::{Router, extract::State, http::HeaderMap, routing::post};
use serde::Deserialize;
use serde_json::{Value, json};
pub fn routes() -> Router<AppState> {
    Router::new().route("/mock/ai/suggest", post(suggest))
}
#[derive(Deserialize)]
struct Input {
    text: String,
}
pub fn suggestions(text: &str) -> Value {
    let mut fields = vec![];
    let mut docs = vec![];
    let mut keys = std::collections::HashSet::new();
    for line in text.lines().take(2000) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let lower = line.to_lowercase();
        if lower.contains("attach") || lower.contains("supporting document") {
            let label = line.trim_matches(['☐', '□', ' ']);
            let key = format!("attachment_{}", docs.len() + 1);
            docs.push(json!({"key":key,"label":label,"required":false,"accept":["application/pdf","image/png","image/jpeg"],"public_candidate":false}));
            continue;
        }
        let checkbox =
            line.contains('☐') || line.contains('□') || lower.starts_with("i declare") || lower.starts_with("i agree");
        let label = if checkbox {
            line.trim_matches(['☐', '□', ' '])
        } else if let Some((label, _)) = line.split_once(':') {
            label.trim()
        } else if let Some((label, _)) = line.split_once("___") {
            label.trim()
        } else {
            continue;
        };
        if label.is_empty()
            || label.chars().count() > 150
            || lower.contains("official use")
            || lower.contains("fee")
            || lower.contains("amount")
            || lower.contains("rate")
        {
            continue;
        }
        let key = label
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("_");
        if !keys.insert(key.clone()) {
            continue;
        }
        let kind = if checkbox {
            "checkbox"
        } else if lower.contains("email") {
            "email"
        } else if lower.contains("phone") || lower.contains("mobile") {
            "phone"
        } else if lower.contains("portion") || lower.contains("lot no") {
            "property_ref"
        } else if lower.contains("date") {
            "date"
        } else {
            "text"
        };
        fields.push(json!({"key":key,"type":kind,"label":label,"required":false}));
    }
    let mut def = crate::services::admin::blank_definition("generic");
    def["summary"] = json!("AI-suggested draft for staff review.");
    def["outcome"] = json!("Council response after review.");
    def["fields"] = json!(fields);
    def["documents"] = json!(docs);
    json!({"definition":def,"warnings":["Every suggested field, document and rule must be checked by staff.","Pricing is empty. Configure prices using the Council schedule.","Deadlines are empty. Configure verified policies before publishing."]})
}
async fn suggest(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Input>,
) -> AppResult<Json<Value>> {
    if !state.cfg.ai_enabled {
        return Err(AppError::not_found());
    }
    super::require_mock_key(&state, &headers)?;
    if input.text.len() > 2_000_000 {
        return Err(AppError::field("text", "The extracted form is too large."));
    }
    Ok(Json(suggestions(&input.text)))
}
#[cfg(test)]
mod tests {
    #[test]
    fn suggestions_do_not_invent_prices_or_deadlines() {
        let d = super::suggestions(
            "Name: ____\nEmail: ____\nPortion No ____\n☐ I agree\nAttach title search\nHourly rate: ____",
        );
        assert_eq!(d["definition"]["fields"].as_array().unwrap().len(), 4);
        assert_eq!(d["definition"]["documents"].as_array().unwrap().len(), 1);
        assert_eq!(d["definition"]["pricing"], serde_json::json!([]));
        assert_eq!(d["definition"]["deadlines"], serde_json::json!([]));
    }
}
