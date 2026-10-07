use super::{
    hooks,
    model::{self, Booking, Slot, Unit},
};
use crate::{
    auth::{Actor, StaffActor},
    authz::Role,
    cases::core::{CaseRow, Visibility},
    db, documents,
    error::{AppError, AppResult},
    finance, pdf,
    state::AppState,
    time,
    web::{Json, Path},
};
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
const BOOKING_ROLES: &[Role] = &[Role::Intake, Role::Manager];
#[derive(Deserialize)]
pub struct Revision {
    expected_revision: i64,
}
#[derive(Deserialize)]
pub struct Move {
    unit_code: String,
    #[serde(alias = "start_at")]
    start: String,
    #[serde(alias = "end_at")]
    end: String,
    reason: String,
    expected_revision: i64,
}
#[derive(Deserialize)]
pub struct Cancel {
    reason: String,
    expected_revision: Option<i64>,
}

pub async fn detail(State(st): State<AppState>, a: Actor, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let mut tx = st.db.acquire().await?;
    let (_, access) = model::read_case(&mut tx, &a, id).await?;
    let b = model::booking(&mut tx, id).await?;
    let u: Unit = sqlx::query_as("SELECT * FROM bookable_units WHERE id=?").bind(b.unit_id).fetch_one(&mut *tx).await?;
    let history:Vec<(i64,String,String,String,String,Option<String>)>=sqlx::query_as("SELECT r.revision,u.name,r.start_at,r.end_at,r.status,r.reason FROM booking_revisions r JOIN bookable_units u ON u.id=r.unit_id WHERE booking_id=? ORDER BY revision DESC").bind(b.id).fetch_all(&mut *tx).await?;
    let mut result = json!({"booking":b,"unit":u,"conditions":model::CONDITIONS,"history":history.into_iter().map(|(revision,unit,start_at,end_at,status,reason)|json!({"revision":revision,"unit":unit,"start_at":start_at,"end_at":end_at,"status":status,"reason":reason})).collect::<Vec<_>>()});
    if access.is_staff() {
        let conflicts = model::conflicts(&mut tx, b.unit_id, &b.start_at, &b.end_at, Some(b.id)).await?;
        result["conflicts"] = json!(model::visible_conflicts(&mut tx, &a, conflicts).await?);
        result["settled"] = json!(finance::api::case_settled(&mut tx, id).await?);
        result["can_manage"] = json!(
            access.can_manage()
                && a.roles_for_service(crate::cases::core::load_case(&mut tx, id).await?.service_id)
                    .iter()
                    .any(|r| BOOKING_ROLES.contains(r))
        );
    }
    Ok(Json(result))
}
/// Atomic allocation and revision, independent of other slice implementations.
pub(super) async fn confirm_allocation(
    tx: &mut SqliteConnection,
    case: i64,
    expected: i64,
    actor: Option<i64>,
    label: &str,
) -> AppResult<Booking> {
    let mut b = model::booking(tx, case).await?;
    if b.revision != expected {
        return Err(AppError::stale_revision());
    }
    if b.status != "requested" {
        return Err(AppError::conflict("Only a requested booking can be confirmed."));
    }
    let u: Unit = sqlx::query_as("SELECT * FROM bookable_units WHERE id=?").bind(b.unit_id).fetch_one(&mut *tx).await?;
    model::unit(tx, &u.code).await?;
    if time::parse(&b.start_at)? <= crate::clock::now() {
        return Err(AppError::conflict("The booking start time has passed."));
    }
    model::occupy_booking(tx, &b, &u, label).await?;
    b.status = "confirmed".into();
    b.revision += 1;
    persist(tx, &b).await?;
    model::revision(tx, &b, actor, "Booking confirmed").await?;
    Ok(b)
}
async fn persist(tx: &mut SqliteConnection, b: &Booking) -> AppResult<()> {
    sqlx::query("UPDATE bookings SET unit_id=?,status=?,start_at=?,end_at=?,revision=?,updated_at=?,confirmation_version_id=NULL WHERE id=?")
        .bind(b.unit_id).bind(&b.status).bind(&b.start_at).bind(&b.end_at).bind(b.revision).bind(time::now_str()).bind(b.id).execute(&mut *tx).await?;
    Ok(())
}
async fn confirmation(
    tx: &mut SqliteConnection,
    st: &AppState,
    a: &Actor,
    c: &crate::cases::core::CaseRow,
    b: &Booking,
) -> AppResult<()> {
    let u: Unit = sqlx::query_as("SELECT * FROM bookable_units WHERE id=?").bind(b.unit_id).fetch_one(&mut *tx).await?;
    let money = finance::api::case_money_summary(tx, c.id).await?;
    let (_, lines) = hooks::venue_lines(tx, &u, &b.start_at, &b.end_at).await?;
    let fees = lines
        .iter()
        .map(|l| format!("{}: AUD ${:.2}", l.description, l.amount_cents as f64 / 100.0))
        .collect::<Vec<_>>()
        .join("\n");
    let bytes = pdf::simple_document(
        "Rawson Hall booking confirmation",
        &[
            ("Reference", c.number.clone().unwrap_or_else(|| c.id.to_string())),
            ("Event", c.title.clone()),
            ("Space", u.name),
            ("Start", time::display_local(time::parse(&b.start_at)?)),
            ("End", time::display_local(time::parse(&b.end_at)?)),
            ("Guests", b.attendees.unwrap_or(0).to_string()),
            ("Booking revision", b.revision.to_string()),
        ],
        &[
            (
                "Fees and bond",
                format!(
                    "{fees}\nConfirmed receipts: AUD ${:.2}. Bond held: AUD ${:.2}. Outstanding: AUD ${:.2}.",
                    money.paid_cents as f64 / 100.0,
                    money.deposits_held_cents as f64 / 100.0,
                    money.outstanding_cents as f64 / 100.0
                ),
            ),
            ("Conditions of hire", model::CONDITIONS.into()),
        ],
    );
    let (_, version) = documents::api::attach_generated(
        tx,
        st,
        c.id,
        "booking_confirmation",
        "Rawson Hall booking confirmation",
        Visibility::Applicant,
        bytes,
        a.db_id(),
    )
    .await?;
    sqlx::query(
        "INSERT INTO booking_confirmations(case_id,booking_id,booking_revision,document_version_id) VALUES(?,?,?,?)",
    )
    .bind(c.id)
    .bind(b.id)
    .bind(b.revision)
    .bind(version)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE bookings SET confirmation_version_id=? WHERE id=?")
        .bind(version)
        .bind(b.id)
        .execute(&mut *tx)
        .await?;
    Ok(())
}
pub async fn confirm(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path(id): Path<i64>,
    Json(v): Json<Revision>,
) -> AppResult<Json<Value>> {
    let mut tx = db::write_tx(&st.db).await?;
    let c = model::manage(&mut tx, &a, id, BOOKING_ROLES).await?;
    let requested = model::booking(&mut tx, id).await?;
    require_confirmation_money(&mut tx, &c, &requested).await?;
    if !finance::api::case_settled(&mut tx, id).await? {
        return Err(AppError::conflict("Hire fees and bond must be received before confirmation."));
    }
    let requested = model::booking(&mut tx, id).await?;
    model::ensure_available(&mut tx, &a, requested.unit_id, &requested.start_at, &requested.end_at, Some(requested.id))
        .await?;
    let b = confirm_allocation(&mut tx, id, v.expected_revision, a.db_id(), &c.title).await?;
    let u: Unit = sqlx::query_as("SELECT * FROM bookable_units WHERE id=?").bind(b.unit_id).fetch_one(&mut *tx).await?;
    let summary = format!(
        "Booking confirmed: {}, {}–{}.",
        u.name,
        time::to_local(time::parse(&b.start_at)?).format("%a %-d %b %Y %H:%M"),
        time::to_local(time::parse(&b.end_at)?).format("%H:%M")
    );
    model::record(&mut tx, a.db_id(), id, "booking.confirmed", &summary, json!({"revision":b.revision})).await?;
    confirmation(&mut tx, &st, &a, &c, &b).await?;
    model::applicant_notice(&mut tx, &c, "Your Rawson Hall booking is confirmed", &summary).await?;
    crate::cases::workflow::try_auto_advance(&mut tx, &st, id).await?;
    tx.commit().await?;
    Ok(Json(json!({"booking":b})))
}
fn moved_slot(b: &Booking, v: &Move) -> Slot {
    Slot {
        unit_code: v.unit_code.clone(),
        start_at: v.start.clone(),
        end_at: v.end.clone(),
        attendees: b.attendees.unwrap_or(1),
    }
}
pub async fn preview(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path(id): Path<i64>,
    Json(v): Json<Move>,
) -> AppResult<Json<Value>> {
    let mut tx = st.db.acquire().await?;
    model::manage(&mut tx, &a, id, BOOKING_ROLES).await?;
    let b = model::booking(&mut tx, id).await?;
    if b.revision != v.expected_revision {
        return Err(AppError::stale_revision());
    }
    let u = model::validate_slot(&mut tx, &moved_slot(&b, &v), st.now()).await?;
    let old: Unit =
        sqlx::query_as("SELECT * FROM bookable_units WHERE id=?").bind(b.unit_id).fetch_one(&mut *tx).await?;
    let (_, old_lines) = hooks::venue_lines(&mut tx, &old, &b.start_at, &b.end_at).await?;
    let (_, new_lines) = hooks::venue_lines(&mut tx, &u, &v.start, &v.end).await?;
    let conflicts = model::conflicts(&mut tx, u.id, &v.start, &v.end, Some(b.id)).await?;
    let conflicts = model::visible_conflicts(&mut tx, &a, conflicts).await?;
    Ok(Json(
        json!({"available":conflicts.is_empty(),"conflicts":conflicts,"old_lines":old_lines,"new_lines":new_lines}),
    ))
}
pub(super) async fn move_allocation(
    tx: &mut SqliteConnection,
    b: &mut Booking,
    u: &Unit,
    s: &Slot,
    actor: Option<i64>,
    reason: &str,
    label: &str,
) -> AppResult<()> {
    if !matches!(b.status.as_str(), "requested" | "confirmed") {
        return Err(AppError::conflict("This booking cannot be rescheduled."));
    }
    let busy = model::conflicts(tx, u.id, &s.start_at, &s.end_at, Some(b.id)).await?;
    if !busy.is_empty() {
        return Err(AppError::conflict(format!(
            "Time conflicts with {}.",
            busy.iter().map(|c| c.reference.as_deref().unwrap_or(&c.label)).collect::<Vec<_>>().join(", ")
        )));
    }
    sqlx::query("UPDATE occupancies SET active=0 WHERE booking_id=? AND active=1").bind(b.id).execute(&mut *tx).await?;
    b.unit_id = u.id;
    b.start_at = time::fmt(time::parse(&s.start_at)?);
    b.end_at = time::fmt(time::parse(&s.end_at)?);
    b.revision += 1;
    if b.status == "confirmed" {
        model::occupy_booking(tx, b, u, label).await?;
    }
    persist(tx, b).await?;
    model::revision(tx, b, actor, reason).await
}
pub async fn reschedule(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path(id): Path<i64>,
    Json(v): Json<Move>,
) -> AppResult<Json<Value>> {
    if v.reason.trim().is_empty() || v.reason.len() > 2000 {
        return Err(AppError::field("reason", "Explain the reschedule (up to 2000 characters)."));
    }
    let mut tx = db::write_tx(&st.db).await?;
    let c = model::manage(&mut tx, &a, id, BOOKING_ROLES).await?;
    let mut b = model::booking(&mut tx, id).await?;
    if b.revision != v.expected_revision {
        return Err(AppError::stale_revision());
    }
    finance::api::require_unconsumed(&mut tx, &st, id).await?;
    let s = moved_slot(&b, &v);
    let u = model::validate_slot(&mut tx, &s, st.now()).await?;
    let old: Unit =
        sqlx::query_as("SELECT * FROM bookable_units WHERE id=?").bind(b.unit_id).fetch_one(&mut *tx).await?;
    let (_, old_lines) = hooks::venue_lines(&mut tx, &old, &b.start_at, &b.end_at).await?;
    let (date, new_lines) = hooks::venue_lines(&mut tx, &u, &s.start_at, &s.end_at).await?;
    let changed = old_lines
        .iter()
        .map(|l| (&l.item_code, l.amount_cents))
        .ne(new_lines.iter().map(|l| (&l.item_code, l.amount_cents)));
    model::ensure_available(&mut tx, &a, u.id, &s.start_at, &s.end_at, Some(b.id)).await?;
    move_allocation(&mut tx, &mut b, &u, &s, a.db_id(), &v.reason, &c.title).await?;
    if changed {
        finance::api::reprice_case(&mut tx, &st, &a, id, date, new_lines, &v.reason).await?;
    }
    if b.status == "confirmed" {
        confirmation(&mut tx, &st, &a, &c, &b).await?;
    }
    let summary = format!(
        "Booking rescheduled to {}, {}–{}. {}",
        u.name,
        time::display_local(time::parse(&b.start_at)?),
        time::display_local(time::parse(&b.end_at)?),
        v.reason
    );
    model::record(
        &mut tx,
        a.db_id(),
        id,
        "booking.rescheduled",
        &summary,
        json!({"revision":b.revision,"fees_changed":changed}),
    )
    .await?;
    model::applicant_notice(&mut tx, &c, "Your booking has changed", &summary).await?;
    model::role_notice(
        &mut tx,
        id,
        "intake",
        "Booking changed: check field tasks",
        "Check the preparation and inspection tasks for the new time.",
    )
    .await?;
    super::tasks::refresh_venue_tasks(&mut tx, &c, a.db_id()).await?;
    tx.commit().await?;
    Ok(Json(json!({"booking":b,"fees_changed":changed})))
}
pub async fn cancel(
    State(st): State<AppState>,
    StaffActor(a): StaffActor,
    Path(id): Path<i64>,
    Json(v): Json<Cancel>,
) -> AppResult<Json<Value>> {
    if v.reason.trim().is_empty() || v.reason.len() > 2000 {
        return Err(AppError::field("reason", "Explain why the booking is cancelled."));
    }
    let mut tx = db::write_tx(&st.db).await?;
    let c = model::manage(&mut tx, &a, id, BOOKING_ROLES).await?;
    let mut b = model::booking(&mut tx, id).await?;
    if v.expected_revision.is_some_and(|r| r != b.revision) {
        return Err(AppError::stale_revision());
    }
    if !matches!(b.status.as_str(), "requested" | "confirmed") {
        return Err(AppError::conflict("This booking cannot be cancelled."));
    }
    sqlx::query("UPDATE occupancies SET active=0 WHERE booking_id=?").bind(b.id).execute(&mut *tx).await?;
    record_cancellation(&mut tx, &b, st.now(), a.db_id(), &v.reason).await?;
    b.status = "cancelled".into();
    b.revision += 1;
    persist(&mut tx, &b).await?;
    model::revision(&mut tx, &b, a.db_id(), &v.reason).await?;
    super::tasks::cancel_venue_tasks(&mut tx, &c, a.db_id(), &v.reason).await?;
    model::record(
        &mut tx,
        a.db_id(),
        id,
        "booking.cancelled",
        &format!("Booking cancelled: {}. Finance will review any refund.", v.reason),
        json!({"revision":b.revision}),
    )
    .await?;
    model::role_notice(&mut tx, id, "finance", "Cancelled booking: review refund", &v.reason).await?;
    model::applicant_notice(&mut tx, &c, "Your booking is cancelled", &v.reason).await?;
    tx.commit().await?;
    Ok(Json(json!({"booking":b})))
}

/// Download a specific immutable booking confirmation, using case access for every request.
pub async fn download_confirmation(
    State(st): State<AppState>,
    a: Actor,
    Path((id, version)): Path<(i64, i64)>,
) -> AppResult<impl axum::response::IntoResponse> {
    let blob = {
        let mut tx = st.db.acquire().await?;
        model::read_case(&mut tx, &a, id).await?;
        sqlx::query_scalar::<_,i64>("SELECT v.blob_id FROM document_versions v JOIN documents d ON d.id=v.document_id WHERE v.id=? AND d.case_id=? AND d.category='booking_confirmation' AND d.visibility='applicant' AND d.generated=1 AND d.disposed_at IS NULL AND EXISTS(SELECT 1 FROM booking_confirmations bc WHERE bc.case_id=d.case_id AND bc.document_version_id=v.id)").bind(version).bind(id).fetch_one(&mut *tx).await?
    };
    let (_, bytes) = crate::storage::read(&st, blob).await?;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "application/pdf"),
            (axum::http::header::CACHE_CONTROL, "private, no-store"),
            (axum::http::header::CONTENT_DISPOSITION, "attachment; filename=booking-confirmation.pdf"),
        ],
        bytes,
    ))
}

async fn require_confirmation_money(tx: &mut SqliteConnection, case: &CaseRow, booking: &Booking) -> AppResult<()> {
    if case.current_step.as_deref() != Some("confirm") {
        return Err(AppError::conflict("Reach the paid booking confirmation step first."));
    }
    let issued: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM invoices WHERE case_id=? AND kind='invoice' AND status='issued')",
    )
    .bind(case.id)
    .fetch_one(&mut *tx)
    .await?;
    if !issued {
        return Err(AppError::conflict("Issued invoices must cover all required hire fees and bond."));
    }
    let unit: Unit =
        sqlx::query_as("SELECT * FROM bookable_units WHERE id=?").bind(booking.unit_id).fetch_one(&mut *tx).await?;
    let (_, required) = hooks::venue_lines(tx, &unit, &booking.start_at, &booking.end_at).await?;
    for line in required {
        let covered: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(l.amount_cents-l.credited_cents),0) FROM finance_line_balances l JOIN invoices i ON i.id=l.invoice_id WHERE i.case_id=? AND i.kind='invoice' AND i.status='issued' AND l.price_item_id=? AND l.kind=?")
            .bind(case.id).bind(line.price_item_id).bind(&line.kind).fetch_one(&mut *tx).await?;
        if covered < line.amount_cents {
            return Err(AppError::conflict("Issued invoices must cover all required hire fees and bond."));
        }
    }
    Ok(())
}

pub(super) async fn record_cancellation(
    tx: &mut SqliteConnection,
    b: &Booking,
    now: chrono::DateTime<chrono::Utc>,
    actor: Option<i64>,
    reason: &str,
) -> AppResult<()> {
    sqlx::query("INSERT INTO booking_cancellations(booking_id,cancelled_at,unused,actor_user_id,reason) VALUES(?,?,?,?,?) ON CONFLICT(booking_id) DO NOTHING")
        .bind(b.id).bind(time::fmt(now)).bind(time::parse(&b.start_at)? > now).bind(actor).bind(reason).execute(tx).await?;
    Ok(())
}
