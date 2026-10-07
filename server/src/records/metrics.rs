//! A metric has one predicate, shared by counts, breakdowns and drill-downs.
use super::common::{require_role, rows};
use crate::{
    auth::Actor,
    authz::{self, Role},
    calendar,
    db::{self, SqlValue},
    error::{AppError, AppResult},
    state::AppState,
    time,
    web::{Json, Path, Query},
};
use axum::{Router, extract::State, routing::get};
use chrono::NaiveTime;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
pub const METRICS: [&str; 11] = [
    "received",
    "open",
    "waiting_on_applicant",
    "completed",
    "refused",
    "withdrawn",
    "cancelled",
    "reopened",
    "unassigned",
    "overdue",
    "due_soon",
];
const OPEN: &str = "c.status IN ('submitted','in_progress','waiting_on_applicant')";
#[derive(Clone, Default, Deserialize)]
pub struct Filters {
    pub from: Option<String>,
    pub to: Option<String>,
    pub service_id: Option<i64>,
    pub owner_id: Option<i64>,
}
struct Period {
    start: String,
    end: String,
    now: String,
    due: String,
}
async fn period(conn: &mut SqliteConnection, state: &AppState, f: &Filters) -> AppResult<Period> {
    let today = time::local_date(state.now());
    let parse = |v: &Option<String>, name: &str, default| match v {
        Some(v) => time::parse_date(v).map_err(|_| AppError::field(name, "Use YYYY-MM-DD.")),
        None => Ok(default),
    };
    let from = parse(&f.from, "from", today - chrono::Duration::days(30))?;
    let to = parse(&f.to, "to", today)?;
    if from > to {
        return Err(AppError::field("to", "The end must be on or after the start."));
    }
    let tomorrow = to.succ_opt().ok_or_else(|| AppError::field("to", "Date is outside the supported range."))?;
    let due = calendar::add_business_days(conn, today, 3).await?;
    Ok(Period {
        start: time::fmt(time::local_to_utc(from, NaiveTime::MIN)),
        end: time::fmt(time::local_to_utc(tomorrow, NaiveTime::MIN)),
        now: time::fmt(state.now()),
        due: time::fmt(time::local_to_utc(due, NaiveTime::from_hms_opt(17, 0, 0).unwrap())),
    })
}
fn predicate(metric: &str, p: &Period) -> AppResult<(String, Vec<SqlValue>)> {
    let range = |column: &str| {
        (
            format!("{column} >= ? AND {column} < ?"),
            vec![SqlValue::Text(p.start.clone()), SqlValue::Text(p.end.clone())],
        )
    };
    let value = match metric {
        "received" => range("c.submitted_at"),
        "open" => (OPEN.into(), vec![]),
        "waiting_on_applicant" => ("c.status='waiting_on_applicant'".into(), vec![]),
        "completed" | "refused" | "withdrawn" | "cancelled" => {
            let (r, b) = range("c.closed_at");
            (
                format!(
                    "c.status='{metric}' AND {r}{}",
                    if metric == "completed" { " AND c.reopened_count=0" } else { "" }
                ),
                b,
            )
        }
        "reopened" => ("c.reopened_count>0".into(), vec![]),
        "unassigned" => (
            format!(
                "{OPEN} AND NOT EXISTS(SELECT 1 FROM case_assignments a WHERE a.case_id=c.id AND a.role='owner' AND a.ended_at IS NULL)"
            ),
            vec![],
        ),
        "overdue" => (
            format!(
                "{OPEN} AND EXISTS(SELECT 1 FROM deadlines d WHERE d.case_id=c.id AND (d.status='breached' OR (d.status='running' AND d.due_at<?)))"
            ),
            vec![SqlValue::Text(p.now.clone())],
        ),
        "due_soon" => (
            format!(
                "{OPEN} AND EXISTS(SELECT 1 FROM deadlines d WHERE d.case_id=c.id AND d.status='running' AND d.due_at>=? AND d.due_at<=?)"
            ),
            vec![SqlValue::Text(p.now.clone()), SqlValue::Text(p.due.clone())],
        ),
        _ => return Err(AppError::not_found()),
    };
    Ok(value)
}
async fn selection(
    conn: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    metric: &str,
    f: &Filters,
) -> AppResult<(String, Vec<SqlValue>)> {
    let p = period(conn, state, f).await?;
    let scope = authz::case_scope_sql(actor);
    let (predicate, binds) = predicate(metric, &p)?;
    let mut all = scope.binds;
    all.extend(binds);
    let mut sql = format!("{} AND ({predicate})", scope.sql);
    if let Some(id) = f.service_id {
        sql.push_str(" AND c.service_id=?");
        all.push(SqlValue::Int(id));
    }
    if let Some(id) = f.owner_id {
        sql.push_str(" AND EXISTS(SELECT 1 FROM case_assignments a WHERE a.case_id=c.id AND a.role='owner' AND a.ended_at IS NULL AND a.user_id=?)");
        all.push(SqlValue::Int(id));
    }
    Ok((sql, all))
}
pub async fn count(
    conn: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    metric: &str,
    f: &Filters,
) -> AppResult<i64> {
    let (predicate, binds) = selection(conn, state, actor, metric, f).await?;
    Ok(db::bind_all_scalar(sqlx::query_scalar(&format!("SELECT COUNT(*) FROM cases c WHERE {predicate}")), &binds)
        .fetch_one(conn)
        .await?)
}
pub async fn cases(
    conn: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    metric: &str,
    f: &Filters,
) -> AppResult<Vec<Value>> {
    let (predicate, binds) = selection(conn, state, actor, metric, f).await?;
    rows(conn,&format!("SELECT c.id,c.number,c.title,c.status,c.applicant_name,c.service_id,s.name AS service_name,c.submitted_at,c.closed_at,c.reopened_count FROM cases c JOIN services s ON s.id=c.service_id WHERE {predicate} ORDER BY c.id"),&binds).await
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/dashboard", get(dashboard))
        .route("/api/staff/dashboard/metrics/{metric}", get(drill))
}
async fn drill(
    State(state): State<AppState>,
    actor: Actor,
    Path(metric): Path<String>,
    Query(f): Query<Filters>,
) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Manager)?;
    let mut conn = state.db.acquire().await?;
    Ok(Json(json!({"metric":metric,"items":cases(&mut conn,&state,&actor,&metric,&f).await?})))
}
async fn dashboard(State(state): State<AppState>, actor: Actor, Query(f): Query<Filters>) -> AppResult<Json<Value>> {
    require_role(&actor, Role::Manager)?;
    let mut tx = state.db.begin().await?; // All counts see the same read snapshot.
    let mut metrics = serde_json::Map::new();
    for metric in METRICS {
        metrics.insert(metric.into(), json!(count(&mut tx, &state, &actor, metric, &f).await?));
    }
    let services: Vec<(i64, String)> =
        sqlx::query_as("SELECT id,name FROM services ORDER BY name").fetch_all(&mut *tx).await?;
    let mut breakdown = Vec::new();
    for (id, name) in services {
        let sf = Filters { service_id: Some(id), ..f.clone() };
        let mut values = serde_json::Map::new();
        for metric in METRICS {
            values.insert(metric.into(), json!(count(&mut tx, &state, &actor, metric, &sf).await?));
        }
        let completed = cases(&mut tx, &state, &actor, "completed", &sf).await?;
        let mut days: Vec<f64> = completed
            .iter()
            .filter_map(|r| {
                Some(
                    (time::parse(r["closed_at"].as_str()?).ok()? - time::parse(r["submitted_at"].as_str()?).ok()?)
                        .num_seconds() as f64
                        / 86400.0,
                )
            })
            .collect();
        days.sort_by(f64::total_cmp);
        let median =
            if days.is_empty() { None } else { Some((days[(days.len() - 1) / 2] + days[days.len() / 2]) / 2.0) };
        breakdown.push(json!({"service_id":id,"service_name":name,"metrics":values,"median_days":median}));
    }
    let staff: Vec<(i64, String)> =
        sqlx::query_as("SELECT id,display_name FROM users WHERE kind='staff' AND is_active=1 ORDER BY display_name")
            .fetch_all(&mut *tx)
            .await?;
    let mut workload = Vec::new();
    for (id, name) in staff {
        let sf = Filters { owner_id: Some(id), ..f.clone() };
        workload.push(json!({"user_id":id,"name":name,"open":count(&mut tx,&state,&actor,"open",&sf).await?,"overdue":count(&mut tx,&state,&actor,"overdue",&sf).await?}));
    }
    tx.commit().await?;
    Ok(Json(json!({"metrics":metrics,"services":breakdown,"workload":workload})))
}
