//! Demo mode: persona login, the demo authenticator and the DemoMail mailbox.
//! Every endpoint answers `not_found` unless `DEMO_MODE=true`.

use axum::Router;
use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::response::{AppendHeaders, IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::audit;
use crate::auth::extract::MaybeSession;
use crate::auth::routes::start_session;
use crate::auth::{UserKind, totp};
use crate::authz::Role;
use crate::db::write_tx;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::web::{ClientIp, Json};

/// A fictional demo persona (ARCHITECTURE §8). `seed` creates exactly these users.
#[derive(Debug, Clone, Copy)]
pub struct Persona {
    pub key: &'static str,
    pub name: &'static str,
    pub kind: UserKind,
    pub job_title: Option<&'static str>,
    pub roles: &'static [Role],
    /// Short description for the persona picker.
    pub description: &'static str,
    pub phone: Option<&'static str>,
}

impl Persona {
    pub fn email(&self) -> String {
        format!("{}@demo.servicehub.invalid", self.key)
    }
}

/// Name of the seeded demo organisation (Ben is its active owner).
pub const DEMO_ORGANISATION: &str = "Island Builders Pty Ltd";

pub const PERSONAS: &[Persona] = &[
    Persona {
        key: "alexey",
        name: "Alexey Turner",
        kind: UserKind::Resident,
        job_title: None,
        roles: &[],
        description: "Resident. Hires Rawson Hall, reports a pothole, asks for a planning certificate.",
        phone: Some("+672 3 51234"),
    },
    Persona {
        key: "ben",
        name: "Ben Carter",
        kind: UserKind::Resident,
        job_title: Some("Director, Island Builders Pty Ltd"),
        roles: &[],
        description: "Business account for Island Builders Pty Ltd. Lodges building applications and hires equipment.",
        phone: Some("+672 3 52345"),
    },
    Persona {
        key: "olga",
        name: "Olga Novak",
        kind: UserKind::Staff,
        job_title: Some("Customer Care Officer"),
        roles: &[Role::Intake],
        description: "Customer Care intake: checks new requests, records phone and counter requests, assigns work.",
        phone: None,
    },
    Persona {
        key: "priya",
        name: "Priya Nair",
        kind: UserKind::Staff,
        job_title: Some("Planning & Building Officer"),
        roles: &[Role::Specialist],
        description: "Assesses planning and building applications; holds decision authority.",
        phone: None,
    },
    Persona {
        key: "tom",
        name: "Tom Becker",
        kind: UserKind::Staff,
        job_title: Some("Finance Officer"),
        roles: &[Role::Finance],
        description: "Invoices, payments, bank statement matching, bonds and refunds.",
        phone: None,
    },
    Persona {
        key: "jake",
        name: "Jake Rowe",
        kind: UserKind::Staff,
        job_title: Some("Works Depot Plant Operator"),
        roles: &[Role::FieldWorker],
        description: "Field worker: sees only tasks assigned to him, records work and equipment hours.",
        phone: None,
    },
    Persona {
        key: "helen",
        name: "Helen Ford",
        kind: UserKind::Staff,
        job_title: Some("Manager, Customer & Community Services"),
        roles: &[Role::Manager],
        description: "Manager: dashboard, reassignment, escalations, grants decision authority.",
        phone: None,
    },
    Persona {
        key: "ruth",
        name: "Ruth Adams",
        kind: UserKind::Staff,
        job_title: Some("Complaints & Governance Officer"),
        roles: &[Role::ComplaintsOfficer],
        description: "Handles confidential complaints and internal reviews.",
        phone: None,
    },
    Persona {
        key: "mark",
        name: "Mark Ellis",
        kind: UserKind::Staff,
        job_title: Some("Systems Administrator"),
        roles: &[Role::Sysadmin],
        description: "Configures services, prices, users and integrations. Sees no case content.",
        phone: None,
    },
];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/demo/personas", get(personas))
        .route("/api/demo/login", post(demo_login))
        .route("/api/demo/authenticator", get(authenticator))
        .route("/api/demo/mailbox", get(mailbox))
}

fn ensure_demo(st: &AppState) -> AppResult<()> {
    if st.cfg.demo_mode { Ok(()) } else { Err(AppError::not_found()) }
}

#[derive(Debug, Serialize)]
struct PersonaOut {
    persona: &'static str,
    name: String,
    kind: UserKind,
    job_title: Option<String>,
    roles: Vec<String>,
    organisation: Option<String>,
    description: &'static str,
}

async fn personas(State(st): State<AppState>) -> AppResult<Json<Vec<PersonaOut>>> {
    ensure_demo(&st)?;
    let mut conn = st.db.acquire().await?;
    let mut out = Vec::new();
    for p in PERSONAS {
        let row: Option<(i64, String, Option<String>)> =
            sqlx::query_as("SELECT id, display_name, job_title FROM users WHERE persona_key = ? AND is_active = 1")
                .bind(p.key)
                .fetch_optional(&mut *conn)
                .await?;
        let Some((id, name, job_title)) = row else { continue };
        let roles: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT role FROM role_grants WHERE user_id = ? AND revoked_at IS NULL ORDER BY role",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
        let organisation: Option<String> = sqlx::query_scalar(
            "SELECT o.name FROM memberships m JOIN organisations o ON o.id = m.organisation_id \
             WHERE m.user_id = ? AND m.status = 'active' ORDER BY m.id LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
        out.push(PersonaOut {
            persona: p.key,
            name,
            kind: p.kind,
            job_title,
            roles,
            organisation,
            description: p.description,
        });
    }
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
struct LoginBody {
    persona: String,
}

async fn demo_login(
    State(st): State<AppState>,
    MaybeSession(prev): MaybeSession,
    ClientIp(ip): ClientIp,
    Json(body): Json<LoginBody>,
) -> AppResult<Response> {
    ensure_demo(&st)?;
    let mut tx = write_tx(&st.db).await?;
    let user_id: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE persona_key = ? AND is_active = 1")
        .bind(&body.persona)
        .fetch_optional(&mut *tx)
        .await?;
    let user_id = user_id.ok_or_else(|| AppError::field("persona", "Unknown demo persona."))?;
    let (cookie, me) = start_session(&st, &mut tx, user_id, prev.as_ref().map(|s| s.token_hash.as_str())).await?;
    audit::record_with_ip(
        &mut tx,
        Some(user_id),
        "auth.demo_login",
        "user",
        Some(user_id),
        json!({ "persona": body.persona }),
        Some(&ip),
    )
    .await?;
    tx.commit().await?;
    Ok((AppendHeaders([(SET_COOKIE, cookie)]), Json(me)).into_response())
}

#[derive(Debug, Serialize)]
struct AuthenticatorCode {
    persona: String,
    name: String,
    code: String,
    seconds_left: u64,
}

async fn authenticator(State(st): State<AppState>) -> AppResult<Json<Vec<AuthenticatorCode>>> {
    ensure_demo(&st)?;
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT persona_key, display_name, totp_secret FROM users \
         WHERE persona_key IS NOT NULL AND kind = 'staff' AND totp_enabled = 1 AND totp_secret IS NOT NULL AND is_active = 1",
    )
    .fetch_all(&st.db)
    .await?;
    let now = st.now().timestamp().max(0) as u64;
    let order = |k: &str| PERSONAS.iter().position(|p| p.key == k).unwrap_or(usize::MAX);
    let mut out = rows
        .into_iter()
        .map(|(persona, name, secret)| {
            Ok(AuthenticatorCode {
                code: totp::code_at(&secret, now)?,
                seconds_left: totp::seconds_left(now),
                persona,
                name,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    out.sort_by_key(|c| order(&c.persona));
    Ok(Json(out))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct MailboxItem {
    id: i64,
    channel: String,
    to: Option<String>,
    subject: String,
    body: String,
    status: String,
    error: Option<String>,
    created_at: String,
    sent_at: Option<String>,
    external_id: Option<String>,
}

async fn mailbox(State(st): State<AppState>) -> AppResult<Json<Vec<MailboxItem>>> {
    ensure_demo(&st)?;
    let items: Vec<MailboxItem> = sqlx::query_as(
        "SELECT id, channel, to_address AS \"to\", subject, body, status, last_error AS error, created_at, sent_at, external_id \
         FROM notifications WHERE channel IN ('email', 'sms') ORDER BY id DESC LIMIT 100",
    )
    .fetch_all(&st.db)
    .await?;
    Ok(Json(items))
}
