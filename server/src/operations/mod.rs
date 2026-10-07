//! Resources, bookings, equipment hire, field tasks and public road issues.
pub mod api;
mod bookings;
mod calendar;
mod equipment;
pub mod hooks;
mod model;
mod tasks;
#[cfg(test)]
mod tests;

use crate::{
    error::{AppError, AppResult},
    state::AppState,
    time,
};
use axum::{
    Router,
    routing::{get, patch, post},
};
use serde_json::Value;
use sqlx::SqliteConnection;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/public/venues/rawson-hall/availability", get(calendar::availability))
        .route("/api/staff/calendar", get(calendar::staff_calendar))
        .route("/api/admin/resources", get(calendar::resources))
        .route("/api/admin/resources/{id}", patch(calendar::update_resource))
        .route("/api/admin/maintenance", post(calendar::maintenance))
        .route("/api/cases/{id}/booking", get(bookings::detail))
        .route("/api/cases/{id}/booking/confirmation/{version}", get(bookings::download_confirmation))
        .route("/api/cases/{id}/booking/confirm", post(bookings::confirm))
        .route("/api/cases/{id}/booking/reschedule", post(bookings::reschedule))
        .route("/api/cases/{id}/booking/reschedule/preview", post(bookings::preview))
        .route("/api/cases/{id}/booking/cancel", post(bookings::cancel))
        .route("/api/field/tasks", get(tasks::list))
        .route("/api/field/tasks/{id}", get(tasks::detail))
        .route("/api/field/tasks/{id}/updates", post(tasks::update))
        .route("/api/field/tasks/{id}/photos/{update_id}", get(tasks::photo))
        .route("/api/field/tasks/{id}/usage", post(equipment::usage))
        .route("/api/cases/{id}/tasks", get(tasks::case_tasks))
        .route("/api/tasks/{id}/assign", post(tasks::assign))
        .route("/api/tasks/{id}/cancel", post(tasks::cancel))
        .route("/api/cases/{id}/equipment", get(equipment::detail))
        .route("/api/cases/{id}/equipment/schedule", post(equipment::schedule))
        .route("/api/cases/{id}/equipment/usage/{uid}/approve", post(equipment::approve))
        .route("/api/staff/operations/resources", get(calendar::staff_resources))
        .route("/api/public/road-issues", get(calendar::road_issues))
        .route("/api/cases/{id}/road-response", get(calendar::road_detail).post(calendar::road_response))
}
pub async fn handle_job(_state: &AppState, kind: &str, _payload: &Value) -> AppResult<()> {
    Err(AppError::internal(format!("Unknown operations job {kind}")))
}

pub async fn seed(tx: &mut SqliteConnection, _state: &AppState) -> AppResult<()> {
    for (code, name, kind, price) in [
        ("RAWSON_MAIN", "Main hall", "space", None),
        ("RAWSON_SUPPER", "Supper room", "space", None),
        ("EXCAVATOR", "Bobcat", "equipment", Some("EQUIP_EXCAVATOR_HOUR")),
        ("BACKHOE", "Volvo Loader", "equipment", Some("EQUIP_BACKHOE_HOUR")),
        ("TIPPER", "Hino Truck", "equipment", Some("EQUIP_TIPPER_HOUR")),
        ("ROLLER", "Cat Steel Drum Roller 8T", "equipment", Some("EQUIP_ROLLER_HOUR")),
    ] {
        sqlx::query("INSERT INTO resources(code,name,kind,venue,description,prep_minutes,cleanup_minutes,price_item_code) VALUES (?,?,?,?,?,?,?,?) ON CONFLICT(code) DO NOTHING")
            .bind(code).bind(name).bind(kind).bind((kind=="space").then_some("Rawson Hall")).bind(if kind=="space"{"Taylors Road, Burnt Pine. Capacity not published; confirm with Council."}else{"Fictional demo fleet item named after the FY2026-27 council plant schedule. Includes Council operator; final bill uses job card."}).bind(if kind=="space"{60}else{0}).bind(if kind=="space"{60}else{0}).bind(price).execute(&mut *tx).await?;
    }
    for (code, name, fee, resources) in [
        ("rawson-main", "Main hall", "HALL_MAIN_DAY", vec!["RAWSON_MAIN"]),
        ("rawson-supper", "Supper room", "HALL_SUPPER_DAY", vec!["RAWSON_SUPPER"]),
        ("rawson-whole", "Whole venue", "HALL_WHOLE_DAY", vec!["RAWSON_MAIN", "RAWSON_SUPPER"]),
    ] {
        sqlx::query("INSERT INTO bookable_units(code,name,venue,description,fee_item_code,deposit_item_code) VALUES (?,?,'Rawson Hall',?,?,'HALL_BOND') ON CONFLICT(code) DO NOTHING").bind(code).bind(name).bind(model::CONDITIONS).bind(fee).execute(&mut *tx).await?;
        for resource in resources {
            sqlx::query("INSERT OR IGNORE INTO bookable_unit_resources(unit_id,resource_id) SELECT u.id,r.id FROM bookable_units u,resources r WHERE u.code=? AND r.code=?").bind(code).bind(resource).execute(&mut *tx).await?;
        }
    }
    sqlx::query("INSERT INTO occupancies(resource_id,source,label,start_at,end_at,created_at) SELECT id,'maintenance','Fictional demo: floor maintenance','2026-11-16T19:00:00.000Z','2026-11-17T00:00:00.000Z',? FROM resources WHERE code='RAWSON_MAIN' AND NOT EXISTS(SELECT 1 FROM occupancies WHERE source='maintenance' AND label='Fictional demo: floor maintenance')").bind(time::now_str()).execute(&mut *tx).await?;
    Ok(())
}
