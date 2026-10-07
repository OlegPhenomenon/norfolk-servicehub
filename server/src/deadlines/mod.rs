//! Durable deadline sweep: cumulative calendar pause caps, then overdue alerts.
pub mod api;
use crate::{
    cases::core::{self, Visibility},
    db,
    error::{AppError, AppResult},
    notify::{self, Notice},
    state::AppState,
    time,
};
use axum::Router;
use serde_json::{Value, json};
pub fn routes() -> Router<AppState> {
    Router::new()
}
pub async fn handle_job(state: &AppState, kind: &str, _payload: &Value) -> AppResult<()> {
    if kind != "deadline.sweep" {
        return Err(AppError::internal("Unknown deadline job."));
    }
    let now = state.now();
    let mut tx = db::write_tx(&state.db).await?;
    let paused:Vec<(i64,i64,i64,String)> = sqlx::query_as("SELECT d.id,d.case_id,d.max_pause_days,p.started_at FROM deadlines d JOIN deadline_pauses p ON p.deadline_id=d.id WHERE d.status='paused' AND p.ended_at IS NULL AND d.max_pause_days IS NOT NULL").fetch_all(&mut *tx).await?;
    for (id, _case_id, cap, start) in paused {
        let used = api::used_pause_days(&mut tx, id, now).await?;
        if used >= cap {
            // End at the precise cap date, even if this sweep was delayed.
            let current = (time::local_date(now) - time::local_date(time::parse(&start)?)).num_days().max(0);
            let previous = used - current;
            let cap_date = time::local_date(time::parse(&start)?) + chrono::Duration::days((cap - previous).max(0));
            let cap_at = time::local_to_utc(cap_date, time::to_local(time::parse(&start)?).time());
            api::resume_one(&mut tx, id, "cap_reached", cap_at).await?;
        }
    }
    let overdue: Vec<(i64, i64, String)> =
        sqlx::query_as("SELECT id,case_id,label FROM deadlines WHERE status='running' AND due_at<?")
            .bind(time::fmt(now))
            .fetch_all(&mut *tx)
            .await?;
    for (id, case_id, label) in overdue {
        sqlx::query("UPDATE deadlines SET status='breached',breached_at=? WHERE id=?")
            .bind(time::fmt(now))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let summary = format!("The {label} deadline has passed.");
        core::append_event(
            &mut tx,
            case_id,
            None,
            "deadline.breached",
            Visibility::Staff,
            &summary,
            json!({"deadline_id":id}),
        )
        .await?;
        crate::audit::record(&mut tx, None, "deadline.breached", "case", Some(case_id), json!({"deadline_id":id}))
            .await?;
        notify_staff(&mut tx, case_id, &summary).await?;
    }
    tx.commit().await?;
    Ok(())
}
async fn notify_staff(tx: &mut sqlx::SqliteConnection, case_id: i64, body: &str) -> AppResult<()> {
    let ids:Vec<i64>=sqlx::query_scalar("SELECT a.user_id FROM case_assignments a JOIN users u ON u.id=a.user_id WHERE a.case_id=? AND a.role='owner' AND a.ended_at IS NULL AND u.is_active=1 UNION SELECT g.user_id FROM role_grants g JOIN users u ON u.id=g.user_id WHERE g.role='manager' AND g.revoked_at IS NULL AND u.is_active=1").bind(case_id).fetch_all(&mut *tx).await?;
    for id in ids {
        // Explicit exclusions still apply to notifications.
        let Some(actor) = crate::auth::Actor::load_recipient(tx, id).await? else { continue };
        if crate::authz::case_access(tx, &actor, case_id).await?.is_staff() {
            notify::send(
                tx,
                Notice {
                    user_id: Some(id),
                    case_id: Some(case_id),
                    subject: "Request deadline".into(),
                    body: body.into(),
                    link: Some(format!("/staff/cases/{case_id}")),
                    ..Notice::default()
                },
            )
            .await?;
        }
    }
    Ok(())
}
