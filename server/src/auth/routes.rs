//! `/api/me`, `/api/auth/*` and `/api/notifications*`.

use axum::Router;
use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{AppendHeaders, IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqliteConnection;

use crate::audit;
use crate::auth::extract::{AuthSession, MaybeSession};
use crate::auth::session::{self, CSRF_COOKIE, SESSION_COOKIE, SESSION_HOURS};
use crate::auth::users::{self, NewUser, UserPublic};
use crate::auth::{Actor, UserKind, password, throttle, totp};
use crate::db::write_tx;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::jobs;
use crate::state::AppState;
use crate::time;
use crate::web::{ClientIp, Json, Path, cookie, get_cookie};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/me", get(me))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/totp", post(totp_verify))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/totp/enroll", post(totp_enroll))
        .route("/api/auth/totp/enroll/confirm", post(totp_enroll_confirm))
        .route("/api/notifications", get(list_notifications))
        .route("/api/notifications/read-all", post(read_all_notifications))
        .route("/api/notifications/{id}/read", post(read_notification))
}

/// Body of `GET /api/me` (also returned by every endpoint that starts or upgrades a session).
#[derive(Debug, Serialize, Deserialize)]
pub struct Me {
    pub user: Option<serde_json::Value>,
    pub roles: Vec<String>,
    pub csrf_token: String,
    pub mfa_required: bool,
    pub demo_mode: bool,
    pub next_reset_at: Option<String>,
    pub ai_enabled: bool,
}

/// Builds the `/api/me` body for `actor` (or an anonymous visitor).
pub async fn build_me(
    state: &AppState,
    conn: &mut SqliteConnection,
    actor: Option<&Actor>,
    csrf: &str,
) -> AppResult<Me> {
    let user: Option<UserPublic> = match actor {
        Some(a) => Some(users::load_public(conn, a.user_id).await?),
        None => None,
    };
    let next_reset_at = if state.cfg.demo_mode {
        jobs::next_demo_reset(conn, state.cfg.demo_reset_hours).await?.map(time::fmt)
    } else {
        None
    };
    Ok(Me {
        user: user.map(serde_json::to_value).transpose()?,
        roles: actor.map(|a| a.role_names().into_iter().map(String::from).collect()).unwrap_or_default(),
        csrf_token: csrf.to_string(),
        mfa_required: actor.is_some_and(|a| a.is_staff() && !a.mfa_passed),
        demo_mode: state.cfg.demo_mode,
        next_reset_at,
        ai_enabled: state.cfg.ai_enabled,
    })
}

fn session_cookie(state: &AppState, token: &str) -> String {
    cookie(SESSION_COOKIE, token, Some(SESSION_HOURS * 3600), true, state.cfg.cookie_secure)
}

/// Creates a fresh session for `user_id` inside `tx` (replacing the caller's previous session, if any)
/// and returns the `Set-Cookie` value plus the `/api/me` body. Shared by login, register and demo login.
pub async fn start_session(
    state: &AppState,
    tx: &mut SqliteConnection,
    user_id: i64,
    previous_session: Option<&str>,
) -> AppResult<(String, Me)> {
    let kind: String =
        sqlx::query_scalar("SELECT kind FROM users WHERE id = ?").bind(user_id).fetch_one(&mut *tx).await?;
    let mfa_passed = UserKind::parse(&kind)? == UserKind::Resident;
    if let Some(prev) = previous_session {
        session::delete(tx, prev).await?;
    }
    let (token, csrf) = session::create(tx, user_id, mfa_passed, state.now()).await?;
    let actor = Actor::load(tx, user_id, mfa_passed).await?;
    let me = build_me(state, tx, Some(&actor), &csrf).await?;
    Ok((session_cookie(state, &token), me))
}

async fn me(State(st): State<AppState>, MaybeSession(s): MaybeSession, headers: HeaderMap) -> AppResult<Response> {
    let mut conn = st.db.acquire().await?;
    if let Some(s) = s {
        let me = build_me(&st, &mut conn, Some(&s.actor), &s.csrf_token).await?;
        return Ok(Json(me).into_response());
    }
    // Anonymous: double-submit CSRF cookie.
    let (csrf, set) = match get_cookie(&headers, CSRF_COOKIE).filter(|c| c.len() == 64) {
        Some(c) => (c, None),
        None => {
            let c = session::random_token();
            let header = cookie(CSRF_COOKIE, &c, None, false, st.cfg.cookie_secure);
            (c, Some(header))
        }
    };
    let me = build_me(&st, &mut conn, None, &csrf).await?;
    Ok(match set {
        Some(h) => (AppendHeaders([(SET_COOKIE, h)]), Json(me)).into_response(),
        None => Json(me).into_response(),
    })
}

#[derive(Debug, Deserialize)]
struct RegisterBody {
    name: String,
    email: String,
    password: String,
}

fn valid_email(e: &str) -> bool {
    let e = e.trim();
    match e.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !e.contains(' ')
                && e.len() <= 254
        }
        None => false,
    }
}

async fn register(
    State(st): State<AppState>,
    MaybeSession(prev): MaybeSession,
    ClientIp(ip): ClientIp,
    Json(body): Json<RegisterBody>,
) -> AppResult<Response> {
    let mut errors = Vec::new();
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        errors.push(("name", "Enter your name (up to 120 characters)."));
    }
    if !valid_email(&body.email) {
        errors.push(("email", "Enter a valid email address."));
    }
    if body.password.chars().count() < 10 {
        errors.push(("password", "Use at least 10 characters."));
    }
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    let pw = body.password.clone();
    let hash = tokio::task::spawn_blocking(move || password::hash(&pw))
        .await
        .map_err(|e| AppError::internal(format!("hash task: {e}")))??;

    let mut tx = write_tx(&st.db).await?;
    let user_id = users::create_user(
        &mut tx,
        NewUser {
            email: body.email.trim().to_lowercase(),
            display_name: name.to_string(),
            phone: None,
            kind: UserKind::Resident,
            password_hash: Some(hash),
            totp_secret: None,
            totp_enabled: false,
            persona_key: None,
            job_title: None,
        },
    )
    .await
    .map_err(|e| {
        if e.code == ErrorCode::Conflict {
            AppError::field("email", "An account with this email already exists.")
        } else {
            e
        }
    })?;
    let (cookie, me) = start_session(&st, &mut tx, user_id, prev.as_ref().map(|s| s.token_hash.as_str())).await?;
    audit::record_with_ip(&mut tx, Some(user_id), "user.register", "user", Some(user_id), json!({}), Some(&ip)).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, AppendHeaders([(SET_COOKIE, cookie)]), Json(me)).into_response())
}

#[derive(Debug, Deserialize)]
struct LoginBody {
    email: String,
    password: String,
}

async fn login(
    State(st): State<AppState>,
    MaybeSession(prev): MaybeSession,
    ClientIp(ip): ClientIp,
    Json(body): Json<LoginBody>,
) -> AppResult<Response> {
    let email = body.email.trim().to_lowercase();
    let keys = vec![format!("email:{email}"), format!("ip:{ip}")];
    let now = st.now();
    {
        let mut conn = st.db.acquire().await?;
        throttle::check(&mut conn, &keys, now).await?;
    }
    let user: Option<(i64, Option<String>)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE email = ? AND is_active = 1")
            .bind(&email)
            .fetch_optional(&st.db)
            .await?;
    let pw = body.password.clone();
    let candidate = user.clone();
    let ok = tokio::task::spawn_blocking(move || match candidate {
        Some((_, Some(h))) => password::verify(&pw, &h),
        _ => {
            password::dummy_verify(&pw);
            false
        }
    })
    .await
    .map_err(|e| AppError::internal(format!("verify task: {e}")))?;

    let mut tx = write_tx(&st.db).await?;
    throttle::record(&mut tx, &keys, ok, now).await?;
    let Some((user_id, _)) = user.filter(|_| ok) else {
        audit::record_with_ip(&mut tx, None, "auth.login_failed", "user", None, json!({ "email": email }), Some(&ip))
            .await?;
        tx.commit().await?;
        return Err(AppError::new(ErrorCode::Unauthorized, "Email or password is incorrect."));
    };
    let (cookie, me) = start_session(&st, &mut tx, user_id, prev.as_ref().map(|s| s.token_hash.as_str())).await?;
    audit::record_with_ip(&mut tx, Some(user_id), "auth.login", "user", Some(user_id), json!({}), Some(&ip)).await?;
    tx.commit().await?;
    Ok((AppendHeaders([(SET_COOKIE, cookie)]), Json(me)).into_response())
}

#[derive(Debug, Deserialize)]
struct CodeBody {
    code: String,
}

async fn totp_verify(
    State(st): State<AppState>,
    AuthSession(s): AuthSession,
    ClientIp(ip): ClientIp,
    Json(body): Json<CodeBody>,
) -> AppResult<Response> {
    if !s.actor.is_staff() {
        return Err(AppError::conflict("Two-factor authentication is only used for staff accounts."));
    }
    let uid = s.actor.user_id;
    let keys = vec![format!("totp:{uid}")];
    let now = st.now();
    let mut tx = write_tx(&st.db).await?;
    if !s.actor.mfa_passed {
        throttle::check(&mut tx, &keys, now).await?;
        let (secret, enabled, last_step): (Option<String>, i64, Option<i64>) =
            sqlx::query_as("SELECT totp_secret, totp_enabled, totp_last_step FROM users WHERE id = ?")
                .bind(uid)
                .fetch_one(&mut *tx)
                .await?;
        let secret = match secret {
            Some(sec) if enabled == 1 => sec,
            _ => return Err(AppError::conflict("Set up two-factor authentication first.")),
        };
        let step = totp::matching_step(&secret, &body.code, now.timestamp().max(0) as u64)?;
        let failure = match step {
            None => Some("That code is not correct. Check your authenticator and try again."),
            Some(st) if last_step.is_some_and(|l| l >= st as i64) => {
                Some("This code has already been used. Wait for the next code.")
            }
            Some(_) => None,
        };
        if let Some(msg) = failure {
            throttle::record(&mut tx, &keys, false, now).await?;
            audit::record_with_ip(&mut tx, Some(uid), "auth.totp_failed", "user", Some(uid), json!({}), Some(&ip))
                .await?;
            tx.commit().await?;
            return Err(AppError::field("code", msg));
        }
        let step = step.expect("checked above") as i64;
        sqlx::query("UPDATE users SET totp_last_step = ? WHERE id = ?").bind(step).bind(uid).execute(&mut *tx).await?;
        session::mark_mfa_passed(&mut tx, &s.token_hash).await?;
        throttle::record(&mut tx, &keys, true, now).await?;
        audit::record_with_ip(&mut tx, Some(uid), "auth.totp", "user", Some(uid), json!({}), Some(&ip)).await?;
    }
    let actor = Actor::load(&mut tx, uid, true).await?;
    let me = build_me(&st, &mut tx, Some(&actor), &s.csrf_token).await?;
    tx.commit().await?;
    Ok(Json(me).into_response())
}

async fn logout(State(st): State<AppState>, MaybeSession(s): MaybeSession) -> AppResult<Response> {
    if let Some(s) = s {
        let mut tx = write_tx(&st.db).await?;
        session::delete(&mut tx, &s.token_hash).await?;
        audit::record(&mut tx, s.actor.db_id(), "auth.logout", "user", Some(s.actor.user_id), json!({})).await?;
        tx.commit().await?;
    }
    let clear = cookie(SESSION_COOKIE, "", Some(0), true, st.cfg.cookie_secure);
    Ok((StatusCode::NO_CONTENT, AppendHeaders([(SET_COOKIE, clear)])).into_response())
}

#[derive(Debug, Serialize)]
struct Enrollment {
    secret: String,
    otpauth_url: String,
    qr_svg: String,
}

async fn totp_enroll(State(st): State<AppState>, AuthSession(s): AuthSession) -> AppResult<Json<Enrollment>> {
    if !s.actor.is_staff() {
        return Err(AppError::conflict("Two-factor authentication is only used for staff accounts."));
    }
    if s.totp_enabled {
        return Err(AppError::conflict("Two-factor authentication is already set up for this account."));
    }
    let secret = totp::generate_secret();
    let mut tx = write_tx(&st.db).await?;
    let email: String =
        sqlx::query_scalar("SELECT email FROM users WHERE id = ?").bind(s.actor.user_id).fetch_one(&mut *tx).await?;
    sqlx::query("UPDATE users SET totp_secret = ? WHERE id = ? AND totp_enabled = 0")
        .bind(&secret)
        .bind(s.actor.user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let otpauth_url = totp::otpauth_url(&secret, &email);
    let qr_svg = totp::qr_svg(&otpauth_url)?;
    Ok(Json(Enrollment { secret, otpauth_url, qr_svg }))
}

async fn totp_enroll_confirm(
    State(st): State<AppState>,
    AuthSession(s): AuthSession,
    ClientIp(ip): ClientIp,
    Json(body): Json<CodeBody>,
) -> AppResult<Response> {
    if !s.actor.is_staff() {
        return Err(AppError::conflict("Two-factor authentication is only used for staff accounts."));
    }
    let uid = s.actor.user_id;
    let keys = vec![format!("totp:{uid}")];
    let now = st.now();
    let mut tx = write_tx(&st.db).await?;
    throttle::check(&mut tx, &keys, now).await?;
    let (secret, enabled): (Option<String>, i64) =
        sqlx::query_as("SELECT totp_secret, totp_enabled FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&mut *tx)
            .await?;
    if enabled == 1 {
        return Err(AppError::conflict("Two-factor authentication is already set up for this account."));
    }
    let secret = secret.ok_or_else(|| AppError::conflict("Start the set-up first."))?;
    let Some(step) = totp::matching_step(&secret, &body.code, now.timestamp().max(0) as u64)? else {
        throttle::record(&mut tx, &keys, false, now).await?;
        tx.commit().await?;
        return Err(AppError::field("code", "That code is not correct. Check your authenticator and try again."));
    };
    sqlx::query("UPDATE users SET totp_enabled = 1, totp_last_step = ? WHERE id = ?")
        .bind(step as i64)
        .bind(uid)
        .execute(&mut *tx)
        .await?;
    session::mark_mfa_passed(&mut tx, &s.token_hash).await?;
    throttle::record(&mut tx, &keys, true, now).await?;
    audit::record_with_ip(&mut tx, Some(uid), "auth.totp_enrolled", "user", Some(uid), json!({}), Some(&ip)).await?;
    let actor = Actor::load(&mut tx, uid, true).await?;
    let me = build_me(&st, &mut tx, Some(&actor), &s.csrf_token).await?;
    tx.commit().await?;
    Ok(Json(me).into_response())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct NotificationItem {
    id: i64,
    subject: String,
    body: String,
    link: Option<String>,
    case_id: Option<i64>,
    created_at: String,
    read_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct NotificationList {
    items: Vec<NotificationItem>,
    unread_count: i64,
}

fn notification_scope(actor: &Actor) -> crate::authz::ScopeSql {
    let mut scope = crate::authz::case_scope_sql(actor);
    scope.sql = format!("(case_id IS NULL OR case_id IN (SELECT c.id FROM cases c WHERE {}))", scope.sql);
    scope
}

async fn list_notifications(State(st): State<AppState>, actor: Actor) -> AppResult<Json<NotificationList>> {
    let mut conn = st.db.acquire().await?;
    let scope = notification_scope(&actor);
    let sql = format!(
        "SELECT id,subject,body,link,case_id,created_at,read_at FROM notifications WHERE {} AND user_id=? AND channel='in_app' ORDER BY id DESC LIMIT 50",
        scope.sql
    );
    let items: Vec<NotificationItem> =
        crate::db::bind_all_as(sqlx::query_as(&sql), &scope.binds).bind(actor.user_id).fetch_all(&mut *conn).await?;
    let sql = format!(
        "SELECT COUNT(*) FROM notifications WHERE {} AND user_id=? AND channel='in_app' AND read_at IS NULL",
        scope.sql
    );
    let unread_count: i64 = crate::db::bind_all_scalar(sqlx::query_scalar(&sql), &scope.binds)
        .bind(actor.user_id)
        .fetch_one(&mut *conn)
        .await?;
    Ok(Json(NotificationList { items, unread_count }))
}

async fn read_notification(State(st): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<StatusCode> {
    let mut tx = write_tx(&st.db).await?;
    let scope = notification_scope(&actor);
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM notifications WHERE {} AND id=? AND user_id=? AND channel='in_app')",
        scope.sql
    );
    let exists: bool = crate::db::bind_all_scalar(sqlx::query_scalar(&sql), &scope.binds)
        .bind(id)
        .bind(actor.user_id)
        .fetch_one(&mut *tx)
        .await?;
    if !exists {
        return Err(AppError::not_found());
    }
    sqlx::query("UPDATE notifications SET status = 'read', read_at = ? WHERE id = ? AND read_at IS NULL")
        .bind(time::fmt(st.now()))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn read_all_notifications(State(st): State<AppState>, actor: Actor) -> AppResult<StatusCode> {
    let mut tx = write_tx(&st.db).await?;
    let scope = notification_scope(&actor);
    let sql = format!(
        "UPDATE notifications SET status='read',read_at=? WHERE user_id=? AND channel='in_app' AND read_at IS NULL AND {}",
        scope.sql
    );
    crate::db::bind_all(sqlx::query(&sql).bind(time::fmt(st.now())).bind(actor.user_id), &scope.binds)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
