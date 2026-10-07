//! HTTP-level tests of sessions, CSRF, demo login and staff TOTP (including replay rejection).

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::state::AppState;

/// Minimal cookie-jar client over the router.
struct Client {
    app: Router,
    cookies: Vec<(String, String)>,
    csrf: String,
}

impl Client {
    fn new(state: &AppState) -> Client {
        Client { app: crate::app::build_router(state.clone()), cookies: Vec::new(), csrf: String::new() }
    }

    async fn send(&mut self, method: &str, path: &str, body: Option<Value>, csrf: bool) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path).header("x-forwarded-for", "198.51.100.7");
        if !self.cookies.is_empty() {
            let c = self.cookies.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ");
            req = req.header(header::COOKIE, c);
        }
        if csrf {
            req = req.header("x-csrf-token", &self.csrf);
        }
        let req = match body {
            Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())).unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        for v in res.headers().get_all(header::SET_COOKIE) {
            let first = v.to_str().unwrap().split(';').next().unwrap();
            let (k, val) = first.split_once('=').unwrap();
            self.cookies.retain(|(name, _)| name != k);
            if !val.is_empty() {
                self.cookies.push((k.to_string(), val.to_string()));
            }
        }
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if let Some(t) = json.get("csrf_token").and_then(Value::as_str) {
            self.csrf = t.to_string();
        }
        (status, json)
    }

    async fn get(&mut self, path: &str) -> (StatusCode, Value) {
        self.send("GET", path, None, false).await
    }

    async fn post(&mut self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send("POST", path, Some(body), true).await
    }
}

async fn seeded() -> (AppState, tempfile::TempDir) {
    let (state, dir) = crate::state::test_support::test_state().await;
    crate::seed::seed_demo(&state).await.unwrap();
    (state, dir)
}

#[tokio::test]
async fn demo_staff_login_requires_totp_and_rejects_replay() {
    let (state, _dir) = seeded().await;
    let mut c = Client::new(&state);

    let (s, me) = c.get("/api/me").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(me["user"], Value::Null);
    assert_eq!(me["demo_mode"], true);
    assert!(me["next_reset_at"].is_string());
    assert_eq!(me["csrf_token"].as_str().unwrap().len(), 64);

    // CSRF is enforced for non-GET /api requests.
    let (s, err) = c.send("POST", "/api/demo/login", Some(json!({"persona": "olga"})), false).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{err}");

    let (s, me) = c.post("/api/demo/login", json!({"persona": "olga"})).await;
    assert_eq!(s, StatusCode::OK, "{me}");
    assert_eq!(me["mfa_required"], true);
    assert_eq!(me["user"]["name"], "Olga Novak");
    assert_eq!(me["roles"], json!(["intake"]));

    // Staff endpoints answer mfa_required until TOTP is verified.
    let (s, err) = c.get("/api/notifications").await;
    assert_eq!((s, err["error"]["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("mfa_required")));

    let (_, codes) = c.get("/api/demo/authenticator").await;
    let code = codes.as_array().unwrap().iter().find(|x| x["persona"] == "olga").unwrap()["code"]
        .as_str()
        .unwrap()
        .to_string();

    let (s, err) = c.post("/api/auth/totp", json!({"code": "000000"})).await;
    if code != "000000" {
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{err}");
    }

    let (s, me) = c.post("/api/auth/totp", json!({"code": code})).await;
    assert_eq!(s, StatusCode::OK, "{me}");
    assert_eq!(me["mfa_required"], false);
    let (s, list) = c.get("/api/notifications").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(list["unread_count"], 0);

    // A new session (log in again) cannot reuse the same code.
    let (_, me) = c.post("/api/demo/login", json!({"persona": "olga"})).await;
    assert_eq!(me["mfa_required"], true);
    let (s, err) = c.post("/api/auth/totp", json!({"code": code})).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(err["error"]["fields"]["code"].as_str().unwrap().contains("already been used"), "{err}");

    // Resident persona: no TOTP.
    let (s, me) = c.post("/api/demo/login", json!({"persona": "alexey"})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(me["mfa_required"], false);
    assert_eq!(me["roles"], json!([]));
    let (s, _) = c.get("/api/notifications").await;
    assert_eq!(s, StatusCode::OK);

    // Logout clears the session.
    let (s, _) = c.post("/api/auth/logout", json!({})).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, me) = c.get("/api/me").await;
    assert_eq!(me["user"], Value::Null);

    // Unknown API path → JSON 404.
    let (s, err) = c.get("/api/does-not-exist").await;
    assert_eq!((s, err["error"]["code"].as_str()), (StatusCode::NOT_FOUND, Some("not_found")));
}

#[tokio::test]
async fn register_password_login_throttle_and_enrolment() {
    let (state, _dir) = seeded().await;
    let mut c = Client::new(&state);
    c.get("/api/me").await;

    let (s, err) = c.post("/api/auth/register", json!({"name": "", "email": "nope", "password": "short"})).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(err["error"]["fields"].as_object().unwrap().len(), 3);

    let (s, me) = c
        .post(
            "/api/auth/register",
            json!({"name": "Rosa Quintal", "email": "Rosa@Example.com", "password": "a long enough password"}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{me}");
    assert_eq!(me["user"]["email"], "rosa@example.com");
    let (s, _) = c
        .post(
            "/api/auth/register",
            json!({"name": "Rosa", "email": "rosa@example.com", "password": "a long enough password"}),
        )
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    c.post("/api/auth/logout", json!({})).await;
    c.get("/api/me").await;
    let (s, _) = c.post("/api/auth/login", json!({"email": "rosa@example.com", "password": "wrong password!"})).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, me) =
        c.post("/api/auth/login", json!({"email": "ROSA@example.com", "password": "a long enough password"})).await;
    assert_eq!(s, StatusCode::OK, "{me}");
    assert_eq!(me["user"]["name"], "Rosa Quintal");

    // Durable throttle: 10 failures in 15 minutes → rate_limited even with the right password.
    c.post("/api/auth/logout", json!({})).await;
    c.get("/api/me").await;
    let mut conn = state.db.acquire().await.unwrap();
    crate::auth::throttle::record(
        &mut conn,
        &(0..9).map(|_| "email:rosa@example.com".to_string()).collect::<Vec<_>>(),
        false,
        state.now(),
    )
    .await
    .unwrap();
    drop(conn);
    let (s, err) =
        c.post("/api/auth/login", json!({"email": "rosa@example.com", "password": "a long enough password"})).await;
    assert_eq!((s, err["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("rate_limited")));

    // Staff without TOTP: enrol, confirm with a code, session upgraded.
    sqlx::query("UPDATE users SET totp_enabled = 0, totp_secret = NULL WHERE persona_key = 'mark'")
        .execute(&state.db)
        .await
        .unwrap();
    let (_, me) = c.post("/api/demo/login", json!({"persona": "mark"})).await;
    assert_eq!((me["mfa_required"].as_bool(), me["user"]["totp_enabled"].as_bool()), (Some(true), Some(false)));
    let (s, err) = c.post("/api/auth/totp", json!({"code": "123456"})).await;
    assert_eq!(s, StatusCode::CONFLICT, "{err}");
    let (s, enrol) = c.post("/api/auth/totp/enroll", json!({})).await;
    assert_eq!(s, StatusCode::OK, "{enrol}");
    assert!(enrol["qr_svg"].as_str().unwrap().contains("<svg"));
    assert!(enrol["otpauth_url"].as_str().unwrap().starts_with("otpauth://totp/"));
    let code = crate::auth::totp::code_at(enrol["secret"].as_str().unwrap(), state.now().timestamp() as u64).unwrap();
    let (s, me) = c.post("/api/auth/totp/enroll/confirm", json!({"code": code})).await;
    assert_eq!(s, StatusCode::OK, "{me}");
    assert_eq!((me["mfa_required"].as_bool(), me["user"]["totp_enabled"].as_bool()), (Some(false), Some(true)));
    let (s, _) = c.post("/api/auth/totp/enroll", json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn notifications_and_mailbox() {
    let (state, _dir) = seeded().await;
    let alexey: i64 =
        sqlx::query_scalar("SELECT id FROM users WHERE persona_key = 'alexey'").fetch_one(&state.db).await.unwrap();
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    crate::notify::send(
        &mut tx,
        crate::notify::Notice {
            user_id: Some(alexey),
            email: Some("alexey@bounce.example".into()),
            phone: Some("+672 3 51234".into()),
            case_id: None,
            subject: "Your booking".into(),
            body: "Confirmed.".into(),
            link: Some("/my".into()),
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let mut c = Client::new(&state);
    c.get("/api/me").await;
    c.post("/api/demo/login", json!({"persona": "alexey"})).await;
    let (_, list) = c.get("/api/notifications").await;
    assert_eq!(list["unread_count"], 1);
    let id = list["items"][0]["id"].as_i64().unwrap();
    let (s, _) = c.post(&format!("/api/notifications/{id}/read"), json!({})).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, list) = c.get("/api/notifications").await;
    assert_eq!(list["unread_count"], 0);
    assert!(list["items"][0]["read_at"].is_string());
    let (s, _) = c.post("/api/notifications/999999/read", json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = c.post("/api/notifications/read-all", json!({})).await;
    assert_eq!(s, StatusCode::NO_CONTENT);

    let (_, mailbox) = c.get("/api/demo/mailbox").await;
    let items = mailbox.as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|m| m["status"] == "queued"));
    let jobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE kind = 'notify.deliver'")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(jobs, 2);

    // Mock gateway: key required, bounce addresses rejected.
    let app = crate::app::build_router(state.clone());
    let send = |key: &str, to: &str| {
        Request::builder()
            .method("POST")
            .uri("/mock/mail/send")
            .header("x-mock-key", key)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"channel": "email", "to": to, "subject": "s", "body": "b"}).to_string()))
            .unwrap()
    };
    assert_eq!(app.clone().oneshot(send("wrong", "a@b.example")).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        app.clone().oneshot(send("test-mock-key", "x@bounce.example")).await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(app.clone().oneshot(send("test-mock-key", "a@b.example")).await.unwrap().status(), StatusCode::OK);
}

#[tokio::test]
async fn demo_endpoints_404_outside_demo_mode_and_headers() {
    let (state, _dir) = seeded().await;
    let mut cfg = (*state.cfg).clone();
    cfg.demo_mode = false;
    let state = AppState { cfg: std::sync::Arc::new(cfg), ..state };
    let app = crate::app::build_router(state);
    let res = app.clone().oneshot(Request::get("/api/demo/personas").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(
        res.headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    assert_eq!(res.headers().get(header::X_CONTENT_TYPE_OPTIONS).unwrap(), "nosniff");
    // SPA fallback without a build: a helpful 404 page, still with security headers.
    let res = app.oneshot(Request::get("/staff").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(res.headers().get(header::REFERRER_POLICY).unwrap(), "same-origin");
}

#[tokio::test]
async fn demo_ended_serves_static_page_and_410() {
    let (state, _dir) = crate::state::test_support::test_state().await;
    let mut cfg = (*state.cfg).clone();
    cfg.demo_ends_at = Some(state.now() - chrono::Duration::hours(1));
    cfg.repo_url = Some("https://example.org/servicehub".into());
    let state = AppState { cfg: std::sync::Arc::new(cfg), ..state };
    let app = crate::app::build_router(state);
    let res = app.clone().oneshot(Request::get("/api/me").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::GONE);
    let res = app.oneshot(Request::get("/my").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("This demonstration has ended") && html.contains("https://example.org/servicehub"));
}
