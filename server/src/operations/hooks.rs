//! Operations hooks invoked by the platform dispatcher.
use super::model::{self, Slot};
use crate::{
    auth::Actor,
    cases::core::CaseRow,
    error::{AppError, AppResult},
    finance::{self, api::QuoteLine},
    services::definition::{FieldDef, FieldType, StepDef},
    state::AppState,
    time,
};
use chrono::NaiveDate;
use serde_json::{Value, json};
use sqlx::SqliteConnection;

pub async fn validate_field(
    tx: &mut SqliteConnection,
    _module: &str,
    field: &FieldDef,
    value: &Value,
) -> AppResult<Option<String>> {
    let result = match field.field_type {
        FieldType::BookingSlot => match serde_json::from_value::<Slot>(value.clone()) {
            Ok(s) => model::validate_slot(tx, &s, crate::clock::now()).await.map(|_| ()),
            Err(_) => Err(AppError::validation_msg("Choose a space, future date, start/end times and guest count.")),
        },
        FieldType::EquipmentRequest => validate_equipment(value).map(|_| ()),
        FieldType::Location => validate_location(value),
        _ => return Ok(None),
    };
    match result {
        Ok(()) => Ok(None),
        Err(e) if e.code == crate::error::ErrorCode::Validation => Ok(Some(if e.fields.is_empty() {
            e.message
        } else {
            e.fields.values().cloned().collect::<Vec<_>>().join(" ")
        })),
        Err(e) => Err(e),
    }
}
pub fn validate_equipment(v: &Value) -> AppResult<(i64, NaiveDate)> {
    model::text(v, "description", 2000)?;
    model::text(v, "site_text", 500)?;
    let hours = v["requested_hours"]
        .as_i64()
        .filter(|h| (1..=240).contains(h))
        .ok_or_else(|| AppError::field("requested_hours", "Enter whole hours between 1 and 240."))?;
    let date = time::parse_date(v["preferred_date"].as_str().unwrap_or(""))
        .map_err(|_| AppError::field("preferred_date", "Choose a valid preferred date."))?;
    if date < time::local_date(crate::clock::now()) {
        return Err(AppError::field("preferred_date", "Choose today or a future date."));
    }
    Ok((hours, date))
}
pub fn validate_location(v: &Value) -> AppResult<()> {
    model::text(v, "description", 500)?;
    let lat = v["lat"].as_f64().filter(|x| (-29.15..=-28.98).contains(x));
    let lng = v["lng"].as_f64().filter(|x| (167.90..=168.01).contains(x));
    if lat.is_none() || lng.is_none() {
        return Err(AppError::field("location", "Choose a location within Norfolk Island."));
    }
    Ok(())
}
async fn answer_of_type(tx: &mut SqliteConnection, c: &CaseRow, a: &Value, t: FieldType) -> AppResult<Value> {
    let def = crate::services::definition::load_for_case(tx, c).await?;
    let key = &def
        .fields
        .iter()
        .find(|f| f.field_type == t)
        .ok_or_else(|| AppError::internal("Operations field missing from definition"))?
        .key;
    Ok(a[key].clone())
}
pub async fn on_submit(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: &CaseRow,
    answers: &Value,
) -> AppResult<()> {
    match case.module.as_str() {
        "venue_booking" => {
            let v = answer_of_type(tx, case, answers, FieldType::BookingSlot).await?;
            let s: Slot =
                serde_json::from_value(v).map_err(|_| AppError::field("slot", "Choose a complete booking slot."))?;
            let u = model::validate_slot(tx, &s, state.now()).await?;
            sqlx::query("INSERT INTO bookings(case_id,unit_id,status,start_at,end_at,attendees,created_at,updated_at) VALUES (?,?,'requested',?,?,?,?,?)")
                .bind(case.id).bind(u.id).bind(time::fmt(time::parse(&s.start_at)?)).bind(time::fmt(time::parse(&s.end_at)?)).bind(s.attendees).bind(time::fmt(state.now())).bind(time::fmt(state.now())).execute(&mut *tx).await?;
            let booking = model::booking(tx, case.id).await?;
            model::revision(tx, &booking, actor.db_id(), "Booking requested; awaiting confirmation").await?;
            model::record(
                tx,
                actor.db_id(),
                case.id,
                "booking.requested",
                "Booking requested; your booking is not yet confirmed.",
                json!({"unit":s.unit_code}),
            )
            .await?;
        }
        "equipment_hire" => {
            let v = answer_of_type(tx, case, answers, FieldType::EquipmentRequest).await?;
            let (hours, date) = validate_equipment(&v)?;
            sqlx::query("INSERT INTO equipment_requests(case_id,description,requested_hours,preferred_date,site_text,created_at) VALUES (?,?,?,?,?,?)")
                .bind(case.id).bind(v["description"].as_str()).bind(hours).bind(date.to_string()).bind(v["site_text"].as_str()).bind(time::fmt(state.now())).execute(&mut *tx).await?;
            // Initial estimate uses the closest available plant; the scheduled item may change it.
            let description = v["description"].as_str().unwrap_or("").to_lowercase();
            let code = if description.contains("roller") {
                "EQUIP_ROLLER_HOUR"
            } else if description.contains("truck") || description.contains("tipper") {
                "EQUIP_TIPPER_HOUR"
            } else if description.contains("backhoe") || description.contains("loader") {
                "EQUIP_BACKHOE_HOUR"
            } else {
                "EQUIP_EXCAVATOR_HOUR"
            };
            let line = finance::api::quote(tx, code, hours * 1000, Some(hours * 60), date).await?;
            finance::api::issue_invoice(tx,state,actor,case.id,"estimate",date,vec![line],Some(format!("Estimate for requested {hours} h; plant subject to scheduling. Final charge uses approved job-card minutes plus agreed expenses."))).await?;
            model::record(
                tx,
                actor.db_id(),
                case.id,
                "equipment.requested",
                "Equipment hire requested; the estimate is not a final invoice.",
                json!({"requested_hours":hours}),
            )
            .await?;
        }
        "road_issue" => {
            let v = answer_of_type(tx, case, answers, FieldType::Location).await?;
            copy_location(tx, case.id, &v, true).await?;
            model::record(
                tx,
                actor.db_id(),
                case.id,
                "road.reported",
                "Road issue reported; its location and progress appear on the public map.",
                json!({}),
            )
            .await?;
        }
        _ => {}
    }
    Ok(())
}
pub async fn copy_location(tx: &mut SqliteConnection, id: i64, v: &Value, public: bool) -> AppResult<()> {
    validate_location(v)?;
    sqlx::query("UPDATE cases SET location_text=?,location_lat=?,location_lng=?,public_map=? WHERE id=?")
        .bind(v["description"].as_str())
        .bind(v["lat"].as_f64())
        .bind(v["lng"].as_f64())
        .bind(i64::from(public))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    Ok(())
}
pub async fn on_step_entered(
    _tx: &mut SqliteConnection,
    _state: &AppState,
    _actor: &Actor,
    _case: &CaseRow,
    _step: &StepDef,
    _run: i64,
) -> AppResult<()> {
    Ok(())
}
pub async fn step_guard(_tx: &mut SqliteConnection, _case: &CaseRow, _step: &StepDef) -> AppResult<Option<String>> {
    Ok(None)
}
pub async fn step_guard_handler(tx: &mut SqliteConnection, case: &CaseRow, handler: &str) -> AppResult<Option<String>> {
    let (pass,reason)=match handler {
        "operations.booking_confirmed"=>(sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM bookings WHERE case_id=? AND status='confirmed')").bind(case.id).fetch_one(&mut *tx).await?,"Confirm the paid booking first."),
        "operations.equipment_scheduled"=>(sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM equipment_requests e WHERE case_id=? AND assigned_resource_id IS NOT NULL AND operator_user_id IS NOT NULL AND scheduled_start IS NOT NULL AND scheduled_end IS NOT NULL AND EXISTS(SELECT 1 FROM occupancies o WHERE o.case_id=e.case_id AND o.source='equipment' AND o.active=1))").bind(case.id).fetch_one(&mut *tx).await?,"Assign equipment, an operator and a time first."),
        "operations.usage_invoiced"=>(sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM equipment_usage u JOIN equipment_requests e ON e.id=u.equipment_request_id JOIN invoices i ON i.id=u.final_invoice_id WHERE e.case_id=? AND u.approved_at IS NOT NULL AND i.kind='invoice' AND i.status='issued')").bind(case.id).fetch_one(&mut *tx).await?,"Finance must approve the job card and issue the final invoice."),
        "operations.deposits_settled"=>(finance::api::deposits_settled(tx,case.id).await?,"Settle the bond first."),
        "operations.road_response"=>(crate::documents::api::letter_issued(tx,case.id,"road_response").await?,"Issue the road response letter first."),
        _=>return Err(AppError::internal(format!("Unknown operations handler {handler}"))),
    };
    Ok((!pass).then(|| reason.into()))
}
pub async fn venue_lines(
    tx: &mut SqliteConnection,
    u: &model::Unit,
    start: &str,
    end: &str,
) -> AppResult<(NaiveDate, Vec<QuoteLine>)> {
    let date = time::local_date(time::parse(start)?);
    let fee = u.fee_item_code.as_deref().ok_or_else(|| AppError::internal("Space has no fee item"))?;
    let bond = u.deposit_item_code.as_deref().ok_or_else(|| AppError::internal("Space has no deposit item"))?;
    let lines = vec![
        finance::api::quote(tx, fee, model::day_count(start, end)? * 1000, None, date).await?,
        finance::api::quote(tx, bond, 1000, None, date).await?,
    ];
    Ok((date, lines))
}
pub async fn pricing_lines(
    tx: &mut SqliteConnection,
    case: &CaseRow,
) -> AppResult<Option<(NaiveDate, Vec<QuoteLine>)>> {
    if case.module == "venue_booking" {
        let b = model::booking(tx, case.id).await?;
        let u: model::Unit =
            sqlx::query_as("SELECT * FROM bookable_units WHERE id=?").bind(b.unit_id).fetch_one(&mut *tx).await?;
        return Ok(Some(venue_lines(tx, &u, &b.start_at, &b.end_at).await?));
    }
    if case.module == "equipment_hire" {
        // Approval issues the final invoice. Generic payment entry must not re-estimate usage.
        let date:Option<String>=sqlx::query_scalar("SELECT i.pricing_date FROM invoices i WHERE i.case_id=? AND i.kind='invoice' AND i.status='issued' ORDER BY id DESC LIMIT 1").bind(case.id).fetch_optional(&mut *tx).await?;
        if let Some(date) = date {
            return Ok(Some((time::parse_date(&date)?, vec![])));
        }
        return Err(AppError::conflict("Approve actual equipment usage before requesting payment."));
    }
    Ok(None)
}

/// Cancel operational work in the same transaction as a cancellation-like case closure.
pub async fn on_case_cancelled(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case_id: i64,
    reason: &str,
) -> AppResult<()> {
    if let Some(mut b) = sqlx::query_as::<_, model::Booking>(
        "SELECT * FROM bookings WHERE case_id=? AND status IN ('requested','confirmed')",
    )
    .bind(case_id)
    .fetch_optional(&mut *tx)
    .await?
    {
        super::bookings::record_cancellation(tx, &b, state.now(), actor.db_id(), reason).await?;
        b.status = "cancelled".into();
        b.revision += 1;
        sqlx::query("UPDATE bookings SET status='cancelled',revision=?,updated_at=? WHERE id=?")
            .bind(b.revision)
            .bind(time::fmt(state.now()))
            .bind(b.id)
            .execute(&mut *tx)
            .await?;
        model::revision(tx, &b, actor.db_id(), reason).await?;
    }
    sqlx::query("UPDATE occupancies SET active=0 WHERE case_id=? AND active=1 AND end_at>?")
        .bind(case_id)
        .bind(time::fmt(state.now()))
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE equipment_requests SET status='cancelled' WHERE case_id=? AND status NOT IN ('completed','cancelled')",
    )
    .bind(case_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE tasks SET status='cancelled',revision=revision+1,completed_at=? WHERE case_id=? AND status NOT IN ('done','cancelled')")
        .bind(time::fmt(state.now())).bind(case_id).execute(&mut *tx).await?;
    model::record(
        tx,
        actor.db_id(),
        case_id,
        "operations.case_cancelled",
        "Reservations released and outstanding operational tasks cancelled.",
        serde_json::json!({"reason":reason}),
    )
    .await?;
    model::role_notice(tx, case_id, "finance", "Cancelled request: review settlement", reason).await
}
