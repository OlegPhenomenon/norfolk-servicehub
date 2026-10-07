//! Deadline policies belong to the frozen submission. Time-aware variants support FixedClock callers.
use crate::{
    calendar,
    cases::core::{self, Visibility},
    error::AppResult,
    services::definition::{self, DeadlineBasis, DeadlinePolicy},
    time,
};
use chrono::{DateTime, Duration, NaiveTime, Utc};
use serde_json::json;
use sqlx::SqliteConnection;

pub async fn due_at(
    tx: &mut SqliteConnection,
    start: DateTime<Utc>,
    policy: &DeadlinePolicy,
) -> AppResult<DateTime<Utc>> {
    let date = time::local_date(start);
    let due = match policy.basis {
        DeadlineBasis::Business => calendar::add_business_days(tx, date, policy.days).await?,
        DeadlineBasis::Calendar => {
            calendar::next_business_day_on_or_after(tx, date + Duration::days(policy.days)).await?
        }
    };
    Ok(time::local_to_utc(due, NaiveTime::from_hms_opt(17, 0, 0).expect("17:00")))
}
pub async fn on_trigger(tx: &mut SqliteConnection, case_id: i64, trigger: &str) -> AppResult<()> {
    on_trigger_at(tx, case_id, trigger, crate::clock::now()).await
}
pub async fn on_trigger_at(
    tx: &mut SqliteConnection,
    case_id: i64,
    trigger: &str,
    now: DateTime<Utc>,
) -> AppResult<()> {
    let case = core::load_case(tx, case_id).await?;
    let def = definition::load_for_case(tx, &case).await?;
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, policy_json FROM deadlines WHERE case_id = ? AND status IN ('running','paused','breached')",
    )
    .bind(case_id)
    .fetch_all(&mut *tx)
    .await?;
    for (id, policy) in rows {
        let policy: DeadlinePolicy = serde_json::from_str(&policy)?;
        if trigger == "closed" || policy.stops.as_deref() == Some(trigger) {
            sqlx::query("UPDATE deadline_pauses SET ended_at = ?, ended_reason = 'staff_resumed' WHERE deadline_id = ? AND ended_at IS NULL").bind(time::fmt(now)).bind(id).execute(&mut *tx).await?;
            sqlx::query("UPDATE deadlines SET status = 'met', met_at = ? WHERE id = ?")
                .bind(time::fmt(now))
                .bind(id)
                .execute(&mut *tx)
                .await?;
            record(
                tx,
                case_id,
                "deadline.met",
                &format!("{}: completed.", policy.label),
                json!({"deadline_id":id,"trigger":trigger}),
            )
            .await?;
        }
    }
    if trigger == "closed" {
        return Ok(());
    }
    for policy in def.deadlines.iter().filter(|p| p.starts == trigger) {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM deadlines WHERE case_id = ? AND kind = ?)")
            .bind(case_id)
            .bind(&policy.kind)
            .fetch_one(&mut *tx)
            .await?;
        if exists {
            continue;
        }
        let due = due_at(tx, now, policy).await?;
        sqlx::query("INSERT INTO deadlines (case_id,kind,label,basis,duration_days,pausable,max_pause_days,started_at,due_at,status,policy_json) VALUES (?,?,?,?,?,?,?,?,?,'running',?)")
            .bind(case_id).bind(&policy.kind).bind(&policy.label).bind(if policy.basis == DeadlineBasis::Business {"business"} else {"calendar"}).bind(policy.days).bind(policy.pausable).bind(policy.max_pause_days).bind(time::fmt(now)).bind(time::fmt(due)).bind(serde_json::to_string(policy)?).execute(&mut *tx).await?;
        record(
            tx,
            case_id,
            "deadline.started",
            &resident_text(&policy.label, "running", &time::fmt(due), 0, policy.max_pause_days)?,
            json!({"kind":policy.kind,"due_at":time::fmt(due)}),
        )
        .await?;
    }
    Ok(())
}
pub async fn pause_for_applicant(
    tx: &mut SqliteConnection,
    case_id: i64,
    message_id: i64,
    reason: &str,
) -> AppResult<()> {
    pause_at(tx, case_id, message_id, reason, crate::clock::now()).await
}
pub async fn pause_at(
    tx: &mut SqliteConnection,
    case_id: i64,
    message_id: i64,
    reason: &str,
    now: DateTime<Utc>,
) -> AppResult<()> {
    let ids: Vec<(i64, Option<i64>)> = sqlx::query_as(
        "SELECT id,max_pause_days FROM deadlines WHERE case_id = ? AND pausable = 1 AND status = 'running'",
    )
    .bind(case_id)
    .fetch_all(&mut *tx)
    .await?;
    for (id, cap) in ids {
        if cap.is_some_and(|cap| cap <= 0) || used_pause_days(tx, id, now).await? >= cap.unwrap_or(i64::MAX) {
            continue;
        }
        sqlx::query("INSERT INTO deadline_pauses (deadline_id,reason,message_id,started_at) VALUES (?,?,?,?)")
            .bind(id)
            .bind(reason)
            .bind(message_id)
            .bind(time::fmt(now))
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE deadlines SET status = 'paused' WHERE id = ?").bind(id).execute(&mut *tx).await?;
        record(
            tx,
            case_id,
            "deadline.paused",
            "Your reply pauses the reply deadline.",
            json!({"deadline_id":id,"message_id":message_id}),
        )
        .await?;
    }
    Ok(())
}
pub async fn used_pause_days(tx: &mut SqliteConnection, id: i64, now: DateTime<Utc>) -> AppResult<i64> {
    let rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT started_at,ended_at FROM deadline_pauses WHERE deadline_id = ?")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    let mut days = 0;
    for (start, end) in rows {
        days += (time::local_date(end.as_deref().map(time::parse).transpose()?.unwrap_or(now))
            - time::local_date(time::parse(&start)?))
        .num_days()
        .max(0);
    }
    Ok(days)
}
pub async fn resume(tx: &mut SqliteConnection, case_id: i64, why: &str) -> AppResult<()> {
    resume_at(tx, case_id, why, crate::clock::now()).await
}
pub async fn resume_at(tx: &mut SqliteConnection, case_id: i64, why: &str, now: DateTime<Utc>) -> AppResult<()> {
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM deadlines WHERE case_id = ? AND status = 'paused'")
        .bind(case_id)
        .fetch_all(&mut *tx)
        .await?;
    for id in ids {
        resume_one(tx, id, why, now).await?;
    }
    Ok(())
}
pub async fn resume_one(tx: &mut SqliteConnection, id: i64, why: &str, now: DateTime<Utc>) -> AppResult<()> {
    let (case_id, basis, old_due, pause_id, start): (i64,String,String,i64,String) = sqlx::query_as("SELECT d.case_id,d.basis,d.due_at,p.id,p.started_at FROM deadlines d JOIN deadline_pauses p ON p.deadline_id=d.id WHERE d.id=? AND p.ended_at IS NULL").bind(id).fetch_one(&mut *tx).await?;
    let start_at = time::parse(&start)?;
    let from = time::local_date(start_at);
    let cap: Option<i64> =
        sqlx::query_scalar("SELECT max_pause_days FROM deadlines WHERE id=?").bind(id).fetch_one(&mut *tx).await?;
    let elapsed = (time::local_date(now) - from).num_days().max(0);
    let previous = used_pause_days(tx, id, now).await? - elapsed;
    let cap_at = cap
        .map(|cap| time::local_to_utc(from + Duration::days((cap - previous).max(0)), time::to_local(start_at).time()));
    let capped = cap_at.is_some_and(|at| now >= at);
    let now = cap_at.filter(|at| now >= *at).unwrap_or(now);
    let why = if capped { "cap_reached" } else { why };
    let to = time::local_date(now);
    let old = time::local_date(time::parse(&old_due)?);
    let due = if basis == "business" {
        let days = calendar::business_days_between(tx, from, to).await?;
        calendar::add_business_days(tx, old, days).await?
    } else {
        calendar::next_business_day_on_or_after(tx, old + Duration::days((to - from).num_days().max(0))).await?
    };
    let new_due = time::fmt(time::local_to_utc(due, NaiveTime::from_hms_opt(17, 0, 0).expect("17:00")));
    sqlx::query("UPDATE deadline_pauses SET ended_at=?, ended_reason=? WHERE id=?")
        .bind(time::fmt(now))
        .bind(why)
        .bind(pause_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE deadlines SET status='running', due_at=? WHERE id=?")
        .bind(&new_due)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    record(
        tx,
        case_id,
        "deadline.resumed",
        &format!("The reply deadline resumed. We will reply by {}.", time::display_local(time::parse(&new_due)?)),
        json!({"deadline_id":id,"old_due_at":old_due,"new_due_at":new_due,"reason":why}),
    )
    .await?;
    if why == "cap_reached" {
        let case = core::load_case(tx, case_id).await?;
        crate::notify::send(
            tx,
            crate::notify::Notice {
                user_id: case.applicant_user_id,
                email: case.applicant_email,
                phone: case.applicant_phone,
                case_id: Some(case_id),
                subject: "Reply clock resumed".into(),
                body: "The maximum pause time has been used. The reply deadline is running again.".into(),
                link: Some(format!("/my/cases/{case_id}")),
            },
        )
        .await?;
        super::notify_staff(tx, case_id, "The applicant's deadline pause cap was reached.").await?;
    }
    Ok(())
}
async fn record(
    tx: &mut SqliteConnection,
    id: i64,
    kind: &str,
    summary: &str,
    data: serde_json::Value,
) -> AppResult<()> {
    core::append_event(tx, id, None, kind, Visibility::Applicant, summary, data.clone()).await?;
    crate::audit::record(tx, None, kind, "case", Some(id), data).await
}

pub fn resident_text(label: &str, status: &str, due: &str, used: i64, cap: Option<i64>) -> AppResult<String> {
    Ok(match status {
        "met" => format!("{label}: completed."),
        "cancelled" | "stopped" => format!("{label}: no longer active."),
        "paused" => {
            format!("{label}: paused while we await your reply ({used} of {} pause days used).", cap.unwrap_or(0))
        }
        "breached" => format!("{label}: overdue since {}.", time::display_local(time::parse(due)?)),
        _ => format!("{label}: due by {}.", time::display_local(time::parse(due)?)),
    })
}
