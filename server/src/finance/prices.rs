use crate::{
    auth::Actor,
    error::{AppError, AppResult},
    state::AppState,
    time,
};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
pub async fn seed(tx: &mut SqliteConnection, _state: &AppState) -> AppResult<()> {
    for (code, name, unit, kind, cents) in [
        ("HALL_MAIN_DAY", "Rawson Hall — Main hall (demo schedule)", "day", "fee", 11500),
        ("HALL_SUPPER_DAY", "Rawson Hall — Supper room (demo schedule)", "day", "fee", 5800),
        ("HALL_WHOLE_DAY", "Rawson Hall — Hall & Supper room (demo schedule)", "day", "fee", 15500),
        ("HALL_BOND", "Rawson Hall — Refundable bond (demo schedule)", "each", "deposit", 25000),
        ("DA_LODGEMENT", "Development lodgement — illustrative demo price", "each", "fee", 57000),
        ("BA_LODGEMENT", "Building lodgement — illustrative demo price", "each", "fee", 57000),
        ("MODIFICATION_FEE", "Basic lapse-date modification (demo schedule)", "each", "fee", 25000),
        (
            "BUILDING_WORKS_FEE",
            "Building development and works fee — scale by estimated cost, from $570 (FY2026-27 demo schedule)",
            "each",
            "fee",
            57000,
        ),
        ("PLANNING_CERT", "Planning Certificate — s.98 Planning Act 2002 (demo schedule)", "each", "fee", 18113),
        ("DRIVEWAY_APPLICATION", "Driveway crossover application — illustrative demo price", "each", "fee", 12500),
        ("RECORD_COPY", "Council record copy — illustrative demo price", "each", "fee", 3500),
        ("EQUIP_EXCAVATOR_HOUR", "Bobcat (demo schedule; mapped plant item)", "hour", "fee", 13500),
        ("EQUIP_BACKHOE_HOUR", "Volvo Loader (demo schedule; mapped plant item)", "hour", "fee", 23000),
        ("EQUIP_TIPPER_HOUR", "Hino Truck (demo schedule)", "hour", "fee", 11000),
        ("EQUIP_ROLLER_HOUR", "Cat Steel Drum Roller 8T with Council operator (demo schedule)", "hour", "fee", 21100),
        ("EQUIP_EXPENSES", "Agreed pass-through expenses — illustrative demo price", "each", "fee", 100),
        (
            "BUILDING_STAGE_INSPECTION",
            "Building inspection — per stage (fees schedule $83.00; demo schedule)",
            "each",
            "fee",
            8300,
        ),
    ] {
        sqlx::query("INSERT INTO price_items(code,name,unit,kind) VALUES(?,?,?,?) ON CONFLICT(code) DO NOTHING")
            .bind(code)
            .bind(name)
            .bind(unit)
            .bind(kind)
            .execute(&mut *tx)
            .await?;
        let id: i64 =
            sqlx::query_scalar("SELECT id FROM price_items WHERE code=?").bind(code).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO price_versions(price_item_id,amount_cents,effective_from,created_at) SELECT ?,?,'2026-07-01',? WHERE NOT EXISTS(SELECT 1 FROM price_versions WHERE price_item_id=?)").bind(id).bind(cents).bind(time::now_str()).bind(id).execute(&mut *tx).await?;
    }
    let id: i64 =
        sqlx::query_scalar("SELECT id FROM price_items WHERE code='HALL_MAIN_DAY'").fetch_one(&mut *tx).await?;
    let future: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM price_versions WHERE price_item_id=? AND effective_from='2027-01-01')",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if !future {
        sqlx::query("UPDATE price_versions SET effective_to='2027-01-01' WHERE price_item_id=? AND effective_to IS NULL AND effective_from<'2027-01-01'").bind(id).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO price_versions(price_item_id,amount_cents,effective_from,created_at) VALUES(?,12000,'2027-01-01',?)").bind(id).bind(time::now_str()).execute(&mut *tx).await?;
    }
    Ok(())
}
pub async fn list(tx: &mut SqliteConnection) -> AppResult<Value> {
    let rows = sqlx::query("SELECT * FROM price_items ORDER BY code").fetch_all(&mut *tx).await?;
    let mut result = Vec::new();
    for r in rows {
        let id: i64 = r.get("id");
        let versions = sqlx::query("SELECT * FROM price_versions WHERE price_item_id=? ORDER BY effective_from")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
        result.push(json!({"id":id,"code":r.get::<String,_>("code"),"name":r.get::<String,_>("name"),"unit":r.get::<String,_>("unit"),"kind":r.get::<String,_>("kind"),"versions":versions.iter().map(|v|json!({"id":v.get::<i64,_>("id"),"amount_cents":v.get::<i64,_>("amount_cents"),"effective_from":v.get::<String,_>("effective_from"),"effective_to":v.get::<Option<String>,_>("effective_to")})).collect::<Vec<_>>()}));
    }
    Ok(
        json!({"schedule_note":"FY2026-27 schedule (demo copy — confirm with Council)","items":result,"fee_scales":super::building_fees::scale_rows(tx).await?}),
    )
}
pub async fn schedule(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    code: &str,
    cents: i64,
    date: &str,
) -> AppResult<()> {
    if cents < 0 {
        return Err(AppError::field("amount_cents", "Enter zero or a positive amount."));
    }
    let start = time::parse_date(date).map_err(|_| AppError::field("effective_from", "Enter a valid date."))?;
    if start < time::local_date(state.now()) {
        return Err(AppError::field("effective_from", "New prices must start today or later."));
    }
    let id: i64 = sqlx::query_scalar("SELECT id FROM price_items WHERE code=?").bind(code).fetch_one(&mut *tx).await?;
    // Split the interval effective on this date, preserving any later scheduled rates.
    let (version,from,until):(i64,String,Option<String>)=sqlx::query_as("SELECT id,effective_from,effective_to FROM price_versions WHERE price_item_id=? AND effective_from<=? AND (effective_to IS NULL OR effective_to>?)")
        .bind(id).bind(date).bind(date).fetch_one(&mut *tx).await?;
    if date <= from.as_str() {
        return Err(AppError::field("effective_from", "A rate already starts on this date. Choose a later date."));
    }
    let used:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM invoice_lines l JOIN invoices i ON i.id=l.invoice_id WHERE l.price_version_id=? AND i.status='issued' AND i.pricing_date>=?)")
        .bind(version).bind(date).fetch_one(&mut *tx).await?;
    if used {
        return Err(AppError::field("effective_from", "Choose a date after issued invoices that use this price."));
    }
    sqlx::query("UPDATE price_versions SET effective_to=? WHERE id=?")
        .bind(date)
        .bind(version)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO price_versions(price_item_id,amount_cents,effective_from,effective_to,created_by,created_at) VALUES(?,?,?,?,?,?)")
        .bind(id).bind(cents).bind(date).bind(until).bind(actor.db_id()).bind(time::fmt(state.now())).execute(&mut *tx).await?;
    crate::audit::record(
        tx,
        actor.db_id(),
        "finance.price_scheduled",
        "price_item",
        Some(id),
        json!({"code":code,"amount_cents":cents,"effective_from":date}),
    )
    .await?;
    Ok(())
}
