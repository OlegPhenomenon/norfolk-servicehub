//! Records administration and confidential feedback.
mod admin;
pub mod api;
pub mod backup;
mod common;
mod complaints;
mod export;
pub mod hooks;
pub mod integrations;
mod legacy;
pub mod metrics;
mod organisations;
mod retention;
#[cfg(test)]
mod tests;

use crate::{
    error::{AppError, AppResult},
    state::AppState,
    time,
};
use axum::Router;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(admin::routes())
        .merge(complaints::routes())
        .merge(export::routes())
        .merge(integrations::routes())
        .merge(legacy::routes())
        .merge(metrics::routes())
        .merge(organisations::routes())
        .merge(retention::routes())
}
pub async fn handle_job(state: &AppState, kind: &str, payload: &Value) -> AppResult<()> {
    match kind {
        "integration.deliver" => {
            integrations::deliver(
                state,
                payload["delivery_id"].as_i64().ok_or_else(|| AppError::internal("Missing delivery_id"))?,
            )
            .await
        }
        "records.backup" => backup::daily_job(state).await,
        _ => Err(AppError::internal(format!("Unknown records job: {kind}"))),
    }
}
/// Called by seed and available to the platform scheduler at each tick.
pub async fn schedule_daily(tx: &mut SqliteConnection, state: &AppState) -> AppResult<()> {
    let day = time::fmt_date(time::local_date(state.now()));
    crate::jobs::enqueue(tx, "records.backup", json!({}), Some(format!("records.backup:{day}")), state.now()).await
}
pub async fn seed(tx: &mut SqliteConnection, state: &AppState) -> AppResult<()> {
    for (code, name) in
        [("content_manager", "Content Manager (mock EDRMS)"), ("civica_altitude", "Civica Altitude (mock finance)")]
    {
        sqlx::query("INSERT INTO external_systems(code,name,base_url) VALUES(?,?,?) ON CONFLICT DO NOTHING")
            .bind(code)
            .bind(name)
            .bind(format!("/mock/records/{code}"))
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO mock_system_state(system_code) VALUES(?) ON CONFLICT DO NOTHING")
            .bind(code)
            .execute(&mut *tx)
            .await?;
    }
    for (class, years, description) in [
        ("default", 7, "Illustrative demonstration policy: general service records"),
        ("building", 25, "Illustrative demonstration policy: planning and building records"),
        ("hire", 7, "Illustrative demonstration policy: hire records"),
        ("complaint", 7, "Illustrative demonstration policy: confidential feedback"),
        ("works", 7, "Illustrative demonstration policy: works requests"),
    ] {
        sqlx::query("INSERT INTO retention_rules(record_class,retain_years,trigger_event,description) VALUES(?,?,'case_closed',?) ON CONFLICT DO NOTHING").bind(class).bind(years).bind(description).execute(&mut *tx).await?;
    }
    for (key, email) in [
        ("notify.customer_care_email", "customer-care@example.invalid"),
        ("notify.finance_email", "finance@example.invalid"),
        ("notify.works_depot_email", "works-depot@example.invalid"),
    ] {
        if crate::settings::get_json(tx, key).await?.is_none() {
            crate::settings::set(tx, key, &email, None).await?;
        }
    }
    schedule_daily(tx, state).await
}
