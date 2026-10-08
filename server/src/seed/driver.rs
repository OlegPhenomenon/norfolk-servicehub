//! Shared endpoint driver for seeded stories and HTTP acceptance tests. No domain table writes.
use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
    middleware::{self, Next},
    response::Response,
};
use chrono::{DateTime, Duration, NaiveTime, Utc};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::{
    AppResult, AppState, app,
    auth::session,
    clock::{self, FixedClock},
    db,
    error::AppError,
    time,
};

#[derive(Clone, Default)]
pub struct Credentials {
    pub cookies: BTreeMap<String, String>,
    pub csrf: String,
    pub user_id: i64,
}

/// Owns a private loopback receiver for real provider callbacks; jobs run explicitly, without sleeps.
pub struct Driver {
    pub state: AppState,
    pub clock: Arc<FixedClock>,
    pub router: Router,
    pub people: BTreeMap<String, Credentials>,
    server: tokio::task::JoinHandle<()>,
    serial: usize,
}
impl Drop for Driver {
    fn drop(&mut self) {
        self.server.abort();
    }
}
async fn clocked(
    axum::extract::State(st): axum::extract::State<AppState>,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    clock::scope(st.clock, next.run(req)).await
}
impl Driver {
    pub async fn new(mut state: AppState, now: DateTime<Utc>) -> AppResult<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let clock = Arc::new(FixedClock::new(now));
        state.clock = clock.clone();
        let mut cfg = (*state.cfg).clone();
        cfg.internal_base_url = format!("http://{}", listener.local_addr()?);
        // Preserve public URLs in issued artifacts; only server-to-server transport uses loopback.
        state.cfg = Arc::new(cfg);
        let router = app::build_router(state.clone()).layer(middleware::from_fn_with_state(state.clone(), clocked));
        let served = router.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, served).await.expect("private mock receiver");
        });
        Ok(Self { state, clock, router, people: BTreeMap::new(), server, serial: 0 })
    }
    pub fn key(&mut self) -> String {
        self.serial += 1;
        format!("story-command-{}", self.serial)
    }
    pub fn instant(&self, day: i64, hour: u32, minute: u32) -> String {
        time::fmt(time::local_to_utc(
            time::local_date(self.state.now()) + Duration::days(day),
            NaiveTime::from_hms_opt(hour, minute, 0).expect("valid time"),
        ))
    }
    pub fn hall(&self, unit: &str, day: i64) -> Value {
        json!({"event_name":"Fictional community celebration", "slot":{"unit_code":unit,"start_at":self.instant(day,10,0),"end_at":self.instant(day,16,0),"attendees":40},"alcohol":"no"})
    }
    pub async fn raw(
        &mut self,
        who: &str,
        method: &str,
        path: &str,
        content_type: &str,
        body: Vec<u8>,
        extra: &[(&str, String)],
    ) -> AppResult<(u16, Vec<u8>)> {
        let credentials = self.people.entry(who.into()).or_default();
        let cookie = credentials.cookies.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ");
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("Cookie", cookie)
            .header("X-CSRF-Token", &credentials.csrf)
            .header("Content-Type", content_type)
            .header("X-Forwarded-For", format!("acceptance-{who}"));
        for (name, value) in extra {
            request = request.header(*name, value);
        }
        let res = self
            .router
            .clone()
            .oneshot(request.body(Body::from(body)).map_err(|e| AppError::internal(e.to_string()))?)
            .await
            .expect("infallible router");
        let status = res.status().as_u16();
        for h in res.headers().get_all("set-cookie") {
            if let Some((name, value)) =
                h.to_str().ok().and_then(|s| s.split(';').next()).and_then(|s| s.split_once('='))
            {
                credentials.cookies.insert(name.into(), value.into());
            }
        }
        let bytes =
            to_bytes(res.into_body(), app::BODY_LIMIT).await.map_err(|e| AppError::internal(e.to_string()))?.to_vec();
        Ok((status, bytes))
    }
    pub async fn expect(&mut self, who: &str, method: &str, path: &str, body: Value, status: u16) -> AppResult<Value> {
        let key = self.key();
        let (actual, raw) = self
            .raw(who, method, path, "application/json", body.to_string().into_bytes(), &[("Idempotency-Key", key)])
            .await?;
        if actual != status {
            return Err(AppError::internal(format!(
                "{who} {method} {path}: expected {status}, got {actual}: {}",
                String::from_utf8_lossy(&raw)
            )));
        }
        let value: Value = if raw.is_empty() { Value::Null } else { serde_json::from_slice(&raw)? };
        if let Some(csrf) = value["csrf_token"].as_str() {
            self.people.get_mut(who).expect("credentials").csrf = csrf.into();
        }
        if let Some(id) = value["user"]["id"].as_i64() {
            self.people.get_mut(who).expect("credentials").user_id = id;
        }
        Ok(value)
    }
    pub async fn req(&mut self, who: &str, method: &str, path: &str, body: Value) -> AppResult<Value> {
        self.expect(who, method, path, body, 200).await
    }
    pub async fn login(&mut self, who: &str) -> AppResult<()> {
        self.req(who, "GET", "/api/me", json!({})).await?;
        let me = self.req(who, "POST", "/api/demo/login", json!({"persona":who})).await?;
        if me["mfa_required"] == true {
            let codes = self.req(who, "GET", "/api/demo/authenticator", json!({})).await?;
            let code =
                codes.as_array().expect("codes").iter().find(|v| v["persona"] == who).expect("persona code")["code"]
                    .clone();
            self.req(who, "POST", "/api/auth/totp", json!({"code":code})).await?;
        }
        Ok(())
    }
    /// Trusted seeding uses the session owner's API; acceptance tests use login + TOTP instead.
    pub async fn seed_sessions(&mut self) -> AppResult<()> {
        self.clear_sessions().await?;
        let users: Vec<(i64, String)> =
            sqlx::query_as("SELECT id,persona_key FROM users WHERE persona_key IS NOT NULL ORDER BY id")
                .fetch_all(&self.state.db)
                .await?;
        let mut tx = db::write_tx(&self.state.db).await?;
        for (id, who) in users {
            let (token, csrf) = session::create(&mut tx, id, true, self.state.now()).await?;
            self.people.insert(
                who,
                Credentials { cookies: BTreeMap::from([("nsh_session".into(), token)]), csrf, user_id: id },
            );
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn clear_sessions(&mut self) -> AppResult<()> {
        let mut tx = db::write_tx(&self.state.db).await?;
        for p in self.people.values() {
            if let Some(token) = p.cookies.get("nsh_session") {
                session::delete(&mut tx, &session::hash_token(token)).await?;
            }
        }
        tx.commit().await?;
        self.people.clear();
        Ok(())
    }
    pub async fn seed_time(&mut self, now: DateTime<Utc>) -> AppResult<()> {
        self.clock.set(now);
        self.seed_sessions().await
    }
    pub async fn detail(&mut self, who: &str, case: i64) -> AppResult<Value> {
        self.req(who, "GET", &format!("/api/cases/{case}"), json!({})).await
    }
    pub async fn revision(&mut self, who: &str, case: i64) -> AppResult<i64> {
        Ok(self.detail(who, case).await?["case"]["revision"].as_i64().expect("case revision"))
    }
    pub async fn action(&mut self, who: &str, case: i64, action: &str) -> AppResult<Value> {
        let revision = self.revision(who, case).await?;
        self.req(
            who,
            "POST",
            &format!("/api/cases/{case}/actions/{action}"),
            json!({"expected_revision":revision,"reason":"Fictional demonstration assessment recorded."}),
        )
        .await
    }
    pub async fn upload(&mut self, who: &str, path: &str, fields: &[(&str, String)], pdf: &[u8]) -> AppResult<Value> {
        let boundary = "servicehub-story-boundary";
        let mut body = Vec::new();
        for (key, value) in fields {
            body.extend_from_slice(
                format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n").as_bytes(),
            );
        }
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"drawing.pdf\"\r\nContent-Type: application/pdf\r\n\r\n").as_bytes());
        body.extend_from_slice(pdf);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let (status, raw) =
            self.raw(who, "POST", path, &format!("multipart/form-data; boundary={boundary}"), body, &[]).await?;
        if status != 200 {
            return Err(AppError::internal(format!("Upload {path}: {status}: {}", String::from_utf8_lossy(&raw))));
        }
        Ok(serde_json::from_slice(&raw)?)
    }
    pub async fn answers(&mut self, who: &str, slug: &str) -> AppResult<(Value, Value)> {
        let def = self.req(who, "GET", &format!("/api/public/services/{slug}"), json!({})).await?["definition"].clone();
        let mut answers = json!({});
        for f in def["fields"].as_array().expect("fields") {
            if !f["required"].as_bool().unwrap_or(false) {
                continue;
            }
            if let Some(cond) = f.get("show_if")
                && answers[cond["field"].as_str().unwrap()] != cond["equals"]
            {
                continue;
            }
            let key = f["key"].as_str().unwrap();
            answers[key] = match f["type"].as_str().unwrap() {
                "checkbox" => json!(true),
                "number" => json!(100),
                "select" => f["options"][0]["value"].clone(),
                "multiselect" => json!([f["options"][0]["value"]]),
                "location" => json!({"lat":-29.04,"lng":167.95,"description":"Fictional pothole on Taylors Road"}),
                "date" => json!(time::local_date(self.state.now()).to_string()),
                "email" => json!("fictional@example.invalid"),
                _ => json!(match key {
                    "applicant_name" =>
                        if who == "alexey" {
                            "Alexey Turner"
                        } else {
                            "Ben Carter"
                        },
                    "postal_address" => "Fictional 44 Taylors Road",
                    "property_ref" => "Portion DEMO-44, Taylors Road",
                    "complaint" => "Confidential feedback about Olga",
                    _ => "Fictional demonstration request",
                }),
            };
        }
        Ok((def, answers))
    }
    pub async fn submit(
        &mut self,
        who: &str,
        slug: &str,
        extra: Value,
        org: Option<i64>,
    ) -> AppResult<(i64, BTreeMap<String, Value>)> {
        let (def, mut answers) = self.answers(who, slug).await?;
        for (key, v) in extra.as_object().expect("extra answers") {
            answers[key] = v.clone();
        }
        let c = self.req(who, "POST", &format!("/api/services/{slug}/drafts"), json!({"applicant_org_id":org})).await?
            ["id"]
            .as_i64()
            .expect("draft id");
        self.req(who, "PUT", &format!("/api/cases/{c}/draft"), json!({"answers":answers})).await?;
        let mut docs = BTreeMap::new();
        let pdf = crate::pdf::simple_document(
            "Fictional drawing A-101",
            &[],
            &[("Plan", "Corrected dimensions. Private phone: SECRET-PHONE-555-0199".into())],
        );
        for doc in def["documents"].as_array().expect("documents") {
            if doc["required"] == true {
                let key = doc["key"].as_str().unwrap();
                let upload = self
                    .upload(
                        who,
                        &format!("/api/cases/{c}/documents"),
                        &[("requirement_key", key.into()), ("title", doc["label"].as_str().unwrap().into())],
                        &pdf,
                    )
                    .await?;
                docs.insert(key.into(), upload);
            }
        }
        self.req(who, "POST", &format!("/api/cases/{c}/submit"), json!({})).await?;
        Ok((c, docs))
    }
    pub async fn money(&mut self, who: &str, case: i64) -> AppResult<Value> {
        self.req(who, "GET", &format!("/api/cases/{case}/money"), json!({})).await
    }
    pub async fn drain(&self) -> AppResult<()> {
        for _ in 0..2000 {
            if !clock::scope(self.state.clock.clone(), crate::jobs::run_once(&self.state)).await? {
                return Ok(());
            }
        }
        Err(AppError::internal("Story jobs did not become idle"))
    }
    pub async fn pay(&mut self, who: &str, case: i64, duplicate: bool) -> AppResult<String> {
        let money = self.money(who, case).await?;
        let invoice = money["invoices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["kind"] == "invoice" && i["outstanding_cents"].as_i64().unwrap_or(0) > 0)
            .ok_or_else(|| AppError::internal("No unpaid invoice"))?;
        let checkout = self
            .req(who, "POST", &format!("/api/cases/{case}/checkout"), json!({"invoice_id":invoice["id"]}))
            .await?["checkout_url"]
            .as_str()
            .expect("checkout URL")
            .to_string();
        let path = checkout.strip_prefix(&self.state.cfg.public_base_url).expect("local checkout");
        let (status, page) = self.raw(who, "GET", path, "text/plain", vec![], &[]).await?;
        if status != 200 || !String::from_utf8_lossy(&page).contains("TEST MODE") {
            return Err(AppError::internal("Missing hosted checkout"));
        }
        let action = if duplicate { "duplicate" } else { "success" };
        let expected = format!("action=\"{path}/{action}\"");
        if !String::from_utf8_lossy(&page).contains(&expected) {
            return Err(AppError::internal("Missing payment form"));
        }
        let before = money["payments"].as_array().unwrap().len();
        let (status, _) = self
            .raw(who, "POST", &format!("{path}/{action}"), "application/x-www-form-urlencoded", vec![], &[])
            .await?;
        if status != 303 {
            return Err(AppError::internal(format!("Checkout did not redirect: {status}")));
        }
        if self.money(who, case).await?["payments"].as_array().unwrap().len() != before {
            return Err(AppError::internal("Redirect posted money before webhook"));
        }
        self.clock.advance(Duration::seconds(4));
        self.drain().await?;
        if self.money(who, case).await?["payments"].as_array().unwrap().len() != before + 1 {
            return Err(AppError::internal("Webhook did not confirm exactly one payment"));
        }
        Ok(path.rsplit('/').next().unwrap().into())
    }
    pub async fn complete_task(&mut self, case: i64, kind: &str) -> AppResult<()> {
        let tasks = self.req("olga", "GET", &format!("/api/cases/{case}/tasks"), json!({})).await?;
        let task = tasks
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["kind"] == kind)
            .ok_or_else(|| AppError::internal(format!("No {kind} task")))?;
        let id = task["id"].as_i64().unwrap();
        let mut updates: Vec<(&str, String)> = task["checklist"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| ("checklist", json!({"key":item["key"],"done":true}).to_string()))
            .collect();
        updates.extend([
            ("result", "Fictional work complete; inspection and cleaning recorded.".into()),
            ("status", "done".into()),
        ]);
        for (kind, body) in updates {
            let task = self.req("jake", "GET", &format!("/api/field/tasks/{id}"), json!({})).await?;
            let command = self.key();
            self.req(
                "jake",
                "POST",
                &format!("/api/field/tasks/{id}/updates"),
                json!({"client_command_id":command,"expected_revision":task["revision"],"kind":kind,"body":body}),
            )
            .await?;
        }
        Ok(())
    }
    pub async fn decision(&mut self, case: i64, kind: &str, evidence: Option<Vec<i64>>) -> AppResult<i64> {
        self.decision_for(case, kind, evidence, None).await
    }
    /// Prepare (Priya), submit and issue (Helen); `supersedes` names the original a modification replaces.
    pub async fn decision_for(
        &mut self,
        case: i64,
        kind: &str,
        evidence: Option<Vec<i64>>,
        supersedes: Option<i64>,
    ) -> AppResult<i64> {
        let templates = self.req("priya", "GET", "/api/decision-templates", json!({})).await?;
        let template = templates.as_array().unwrap().iter().find(|t| t["decision_type"] == kind).expect("template");
        let revision = self.revision("priya", case).await?;
        let id = self.req("priya","POST",&format!("/api/cases/{case}/decisions"),json!({"decision_type":kind,"outcome":"approved","reasons":"Fictional specialist assessment completed.","conditions":"Follow approved drawing A-101 v2.","template_id":template["id"],"evidence_version_ids":evidence,"supersedes_decision_id":supersedes,"expected_revision":revision})).await?["id"].as_i64().unwrap();
        for action in ["submit", "issue"] {
            let person = if action == "issue" { "helen" } else { "priya" };
            let revision = self.revision(person, case).await?;
            self.req(
                person,
                "POST",
                &format!("/api/cases/{case}/decisions/{id}/{action}"),
                json!({"expected_revision":revision}),
            )
            .await?;
        }
        Ok(id)
    }
}
