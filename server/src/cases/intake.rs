//! Assisted intake uses the same draft and submission functions as online applications.
use super::{
    drafts::{self, Applicant},
    submission,
};
use crate::{
    auth::StaffActor,
    authz::Role,
    db,
    error::{AppError, AppResult},
    idempotency::{self, IdempotencyKey},
    state::AppState,
    web::Json,
};
use axum::{Router, extract::State, http::StatusCode, routing::post};
use serde::Deserialize;
use serde_json::{Value, json};
pub fn routes() -> Router<AppState> {
    Router::new().route("/api/staff/intake", post(intake))
}
#[derive(Deserialize)]
struct Intake {
    service: String,
    #[serde(flatten)]
    applicant: Applicant,
    answers: Value,
    #[serde(default)]
    draft_only: bool,
    case_id: Option<i64>,
}
async fn intake(
    State(state): State<AppState>,
    StaffActor(actor): StaffActor,
    IdempotencyKey(key): IdempotencyKey,
    Json(raw): Json<Value>,
) -> AppResult<Json<Value>> {
    actor.require_any_role(&[Role::Intake])?;
    let input: Intake = serde_json::from_value(raw.clone()).map_err(|e| AppError::validation_msg(e.to_string()))?;
    if input.applicant.channel.as_deref().is_none_or(|c| !["phone", "walk_in", "email", "post"].contains(&c)) {
        return Err(AppError::field("channel", "Choose phone, walk in, email or post."));
    }
    let mut tx = db::write_tx(&state.db).await?;
    let key = key.ok_or_else(|| AppError::field("idempotency_key", "Send an Idempotency-Key for intake."))?;
    let hash = idempotency::json_hash(&raw);
    if let Some(previous) = idempotency::lookup(&mut tx, actor.user_id, "case.intake", &key, &hash).await? {
        let id = previous.body["id"].as_i64().ok_or_else(|| AppError::internal("Intake replay has no case ID"))?;
        crate::authz::require_staff_case(&mut tx, &actor, id).await?;
        return Ok(Json(previous.body));
    }
    let case = if let Some(id) = input.case_id {
        let case = super::require_edit(&mut tx, &actor, id).await?;
        if case.recorded_by_user_id != actor.db_id() {
            return Err(AppError::forbidden());
        }
        let slug: String = sqlx::query_scalar("SELECT slug FROM services WHERE id=?")
            .bind(case.service_id)
            .fetch_one(&mut *tx)
            .await?;
        if slug != input.service {
            return Err(AppError::conflict("The service cannot change on an existing draft."));
        }
        case
    } else {
        drafts::create_draft(&mut tx, &actor, &input.service, input.applicant).await?
    };
    drafts::save_answers(&mut tx, &actor, case.id, &input.answers).await?;
    let result = if input.draft_only {
        json!(case)
    } else {
        json!(submission::submit_case(&mut tx, &state, &actor, case.id).await?)
    };
    idempotency::store(&mut tx, actor.user_id, "case.intake", &key, &hash, StatusCode::OK, &result).await?;
    tx.commit().await?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn idempotent_intake_replay_rechecks_current_case_access() {
        let (state, _dir) = crate::state::test_support::test_state().await;
        crate::seed::seed_base(&state).await.unwrap();
        let mut c = state.db.acquire().await.unwrap();
        let uid = sqlx::query_scalar("SELECT id FROM users WHERE persona_key='olga'").fetch_one(&mut *c).await.unwrap();
        let actor = crate::auth::Actor::load(&mut c, uid, true).await.unwrap();
        drop(c);
        let input = json!({"service":"planning-certificate","channel":"phone","applicant_name":"Offline applicant","applicant_email":"offline@example.test","draft_only":true,"answers":{}});
        let Json(first) = intake(
            State(state.clone()),
            StaffActor(actor.clone()),
            IdempotencyKey(Some("intake-replay".into())),
            Json(input.clone()),
        )
        .await
        .unwrap();
        let id = first["id"].as_i64().unwrap();
        let Json(replay) = intake(
            State(state.clone()),
            StaffActor(actor.clone()),
            IdempotencyKey(Some("intake-replay".into())),
            Json(input.clone()),
        )
        .await
        .unwrap();
        assert_eq!(first, replay);
        sqlx::query("INSERT INTO case_access_denials(case_id,user_id,reason,created_at) VALUES(?,?,'Access withdrawn','2026-10-07')").bind(id).bind(uid).execute(&state.db).await.unwrap();
        let err = intake(State(state), StaffActor(actor), IdempotencyKey(Some("intake-replay".into())), Json(input))
            .await
            .unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::NotFound);
    }
}
