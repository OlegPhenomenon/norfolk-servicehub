use super::{
    admin::{self, NewService},
    definition::ServiceDefinition,
    validation,
};
use crate::{
    auth::{Actor, RoleGrant, UserKind},
    authz::{self, CaseAccess, Role},
    cases::{
        self,
        core::{self, Visibility},
    },
    db,
    error::ErrorCode,
    state::{AppState, test_support},
    time,
};
use serde_json::{Value, json};
const T0: &str = "2026-10-09T00:00:00Z";
struct Fixture {
    state: AppState,
    clock: std::sync::Arc<crate::clock::FixedClock>,
    _dir: tempfile::TempDir,
    resident: Actor,
    intake: Actor,
    specialist: Actor,
    admin: Actor,
    service: i64,
    version: i64,
}
fn actor(id: i64, kind: UserKind, role: Option<Role>) -> Actor {
    Actor {
        user_id: id,
        kind,
        roles: role.into_iter().map(|role| RoleGrant { role, scope_service_id: None }).collect(),
        display_name: format!("Fictional {id}"),
        mfa_passed: true,
    }
}
async fn fixture(required_doc: bool) -> Fixture {
    let (state, clock, dir) = test_support::test_state_fixed(time::parse(T0).unwrap()).await;
    let mut tx = db::write_tx(&state.db).await.unwrap();
    for (id, kind) in [(1, "resident"), (2, "staff"), (3, "staff"), (4, "staff"), (5, "resident")] {
        sqlx::query("INSERT INTO users(id,email,display_name,kind,created_at) VALUES (?,?,?, ?,?)")
            .bind(id)
            .bind(format!("u{id}@fictional.invalid"))
            .bind(format!("Fictional {id}"))
            .bind(kind)
            .bind(T0)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    for (uid, role) in [(2, "intake"), (3, "specialist"), (4, "sysadmin")] {
        sqlx::query("INSERT INTO role_grants(user_id,role,granted_at) VALUES (?,?,?)")
            .bind(uid)
            .bind(role)
            .bind(T0)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let admin = actor(4, UserKind::Staff, Some(Role::Sysadmin));
    let service = admin::create_service(
        &mut tx,
        &admin,
        &NewService {
            slug: "fixture".into(),
            name: "Fictional service".into(),
            category: "Information".into(),
            module: "generic".into(),
            department: "Customer Care".into(),
        },
    )
    .await
    .unwrap();
    let definition = definition(required_doc);
    let version = admin::create_version(&mut tx, &admin, service, definition, None).await.unwrap();
    admin::publish(&mut tx, &admin, service, version).await.unwrap();
    sqlx::query("INSERT INTO holidays(date,name,source) VALUES ('2026-10-12','Fictional test holiday','demo')")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    Fixture {
        state,
        clock,
        _dir: dir,
        resident: actor(1, UserKind::Resident, None),
        intake: actor(2, UserKind::Staff, Some(Role::Intake)),
        specialist: actor(3, UserKind::Staff, Some(Role::Specialist)),
        admin,
        service,
        version,
    }
}
fn definition(required_doc: bool) -> Value {
    json!({"module":"generic","summary":"Fictional service","outcome":"Written reply","fields":[{"key":"name","type":"text","label":"Original label","required":true},{"key":"extra","type":"select","label":"More details?","options":[{"value":"yes","label":"Yes"},{"value":"no","label":"No"}],"required":true},{"key":"detail","type":"text","label":"Conditional details","required":true,"show_if":{"field":"extra","equals":"yes"}}],"documents":if required_doc {json!([{"key":"plan","label":"Site plan","required":true}])}else{json!([])},"workflow":{"steps":[{"key":"intake","kind":"review","role":"intake","label":"Check request","applicant_label":"Checking your request"},{"key":"assessment","kind":"review","role":"specialist","optional":true,"label":"Assessment","applicant_label":"Assessing your request"},{"key":"done","kind":"complete","label":"Done"}]},"deadlines":[{"kind":"completeness","label":"Completeness","days":1,"basis":"business","starts":"submitted","stops":"step:assessment","pausable":false},{"kind":"response","label":"Reply","days":3,"basis":"business","starts":"submitted","stops":"closed","pausable":true,"max_pause_days":5}]})
}
async fn draft(f: &Fixture) -> i64 {
    let mut tx = db::write_tx(&f.state.db).await.unwrap();
    let c = cases::drafts::create_draft(&mut tx, &f.resident, "fixture", Default::default()).await.unwrap();
    cases::drafts::save_answers(
        &mut tx,
        &f.resident,
        c.id,
        &json!({"name":"Alexey Fictional","extra":"no","detail":"hidden secret","unknown":"ignored"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    c.id
}
#[tokio::test]
async fn validates_structure_references_catalogue_and_hidden_answers() {
    let f = fixture(false).await;
    let mut tx = f.state.db.acquire().await.unwrap();
    let def = ServiceDefinition::parse(&definition(false).to_string()).unwrap();
    assert!(validation::validate_definition(&mut tx, &def).await.unwrap().is_empty());
    let mut invalid = definition(false);
    invalid["fields"][1]["key"] = json!("name");
    invalid["fields"][2]["show_if"]["field"] = json!("future");
    invalid["workflow"]["steps"][1]["kind"] = json!("task");
    invalid["workflow"]["steps"][1]["task_kind"] = json!("unregistered");
    invalid["deadlines"][0]["starts"] = json!("step:missing");
    invalid["pricing"] = json!([{"item":"DOES_NOT_EXIST","quantity":0}]);
    let invalid = ServiceDefinition::parse(&invalid.to_string()).unwrap();
    let issues = validation::validate_definition(&mut tx, &invalid).await.unwrap();
    assert!(issues.len() >= 6);
    assert!(issues.iter().any(|i| i.path == "pricing.0.item"));
    let clean = validation::validate_answers(
        &mut tx,
        "generic",
        &def,
        &json!({"name":"A","extra":"no","detail":"should vanish","unknown":"should vanish"}),
    )
    .await
    .unwrap();
    assert!(clean.get("detail").is_none() && clean.get("unknown").is_none());
    let err =
        validation::validate_answers(&mut tx, "generic", &def, &json!({"name":"A","extra":"yes"})).await.unwrap_err();
    assert!(err.fields.contains_key("detail"));
    assert!(ServiceDefinition::parse(&definition(false).to_string().replace("\"text\"", "\"script\"")).is_err());
}
#[tokio::test]
async fn concurrent_submission_is_one_number_one_snapshot_and_v2_cannot_change_it() {
    let f = fixture(false).await;
    let id = draft(&f).await;
    let (first, second) = tokio::join!(
        cases::submission::submit_idempotent(&f.state, &f.resident, id, "same-key"),
        cases::submission::submit_idempotent(&f.state, &f.resident, id, "same-key")
    );
    assert_eq!(first.unwrap(), second.unwrap());
    let mut tx = db::write_tx(&f.state.db).await.unwrap();
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM submissions").fetch_one(&mut *tx).await.unwrap(), 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM case_events WHERE kind='submitted'")
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM workflow_step_runs").fetch_one(&mut *tx).await.unwrap(),
        1
    );
    let mut v2 = definition(false);
    v2["fields"][0]["label"] = json!("New changed label");
    v2["fields"][0]["required"] = json!(false);
    let version2 = admin::create_version(&mut tx, &f.admin, f.service, v2, None).await.unwrap();
    admin::publish(&mut tx, &f.admin, f.service, version2).await.unwrap();
    let case = core::load_case(&mut tx, id).await.unwrap();
    assert_eq!(case.service_version_id, f.version);
    let frozen = super::definition::load_for_case(&mut tx, &case).await.unwrap();
    assert_eq!(frozen.fields[0].label, "Original label");
    assert!(admin::editable(&mut tx, f.service, f.version).await.is_err());
    let answers: String = sqlx::query_scalar("SELECT answers_json FROM submissions WHERE case_id=?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(serde_json::from_str::<Value>(&answers).unwrap().get("detail").is_none());
    assert!(
        sqlx::query("UPDATE submissions SET answers_json='{}' WHERE case_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn required_document_enforcement_and_latest_version_is_frozen() {
    let f = fixture(true).await;
    let id = draft(&f).await;
    let err = cases::submission::submit_idempotent(&f.state, &f.resident, id, "submit").await.unwrap_err();
    assert!(err.fields.contains_key("documents.plan"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM submissions").fetch_one(&f.state.db).await.unwrap(),
        0
    );
    let mut tx = db::write_tx(&f.state.db).await.unwrap();
    // Direct rows isolate S1's invariant from the documents slice's upload implementation.
    let blob:i64=sqlx::query_scalar("INSERT INTO blobs(sha256,size_bytes,mime,original_name,scan_status,created_at) VALUES ('abc',10,'application/pdf','plan.pdf','clean',?) RETURNING id").bind(T0).fetch_one(&mut *tx).await.unwrap();
    let doc:i64=sqlx::query_scalar("INSERT INTO documents(case_id,requirement_key,category,title,visibility,created_at) VALUES (?,'plan','plans','Site plan','applicant',?) RETURNING id").bind(id).bind(T0).fetch_one(&mut *tx).await.unwrap();
    for version in 1..=2 {
        sqlx::query("INSERT INTO document_versions(document_id,version,blob_id,uploaded_at) VALUES (?,?,?,?)")
            .bind(doc)
            .bind(version)
            .bind(blob)
            .bind(T0)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    cases::submission::submit_idempotent(&f.state, &f.resident, id, "submit").await.unwrap();
    let version: i64 = sqlx::query_scalar(
        "SELECT v.version FROM submission_documents s JOIN document_versions v ON v.id=s.document_version_id",
    )
    .fetch_one(&f.state.db)
    .await
    .unwrap();
    assert_eq!(version, 2);
}
#[tokio::test]
async fn step_roles_revisions_and_internal_projection() {
    let f = fixture(false).await;
    let id = draft(&f).await;
    cases::submission::submit_idempotent(&f.state, &f.resident, id, "submit").await.unwrap();
    let mut tx = db::write_tx(&f.state.db).await.unwrap();
    let case = core::load_case(&mut tx, id).await.unwrap();
    let def = super::definition::load_for_case(&mut tx, &case).await.unwrap();
    assert_eq!(cases::workflow::allowed_actions(&f.resident, &case, CaseAccess::Applicant, &def), vec!["withdraw"]);
    assert!(
        !cases::workflow::allowed_actions(&f.specialist, &case, CaseAccess::Staff { can_manage: true }, &def)
            .contains(&"advance")
    );
    let err = cases::workflow::advance(&mut tx, &f.state, &f.specialist, id, case.revision, None).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    let err = cases::workflow::advance(&mut tx, &f.state, &f.intake, id, case.revision - 1, None).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::StaleRevision);
    let updated = cases::workflow::advance(&mut tx, &f.state, &f.intake, id, case.revision, None).await.unwrap();
    assert_eq!(updated.current_step.as_deref(), Some("assessment"));
    assert!(
        cases::workflow::allowed_actions(&f.specialist, &updated, CaseAccess::Staff { can_manage: true }, &def)
            .contains(&"skip")
    );
    sqlx::query(
        "INSERT INTO internal_notes(case_id,author_user_id,body,created_at) VALUES (?,2,'SECRET INTERNAL NOTE',?)",
    )
    .bind(id)
    .bind(T0)
    .execute(&mut *tx)
    .await
    .unwrap();
    core::append_event(&mut tx, id, Some(2), "note.created", Visibility::Staff, "SECRET INTERNAL EVENT", json!({}))
        .await
        .unwrap();
    let applicant = cases::workflow::projection(&mut tx, &f.state, &f.resident, id).await.unwrap();
    assert!(!applicant.to_string().contains("SECRET"));
    assert!(applicant.get("assignments").is_none());
    let staff = cases::workflow::projection(&mut tx, &f.state, &f.intake, id).await.unwrap();
    assert!(staff.to_string().contains("SECRET INTERNAL EVENT"));
    let applicant_thread = cases::messages::thread(&mut tx, &f.resident, id).await.unwrap();
    assert!(!applicant_thread.to_string().contains("SECRET"));
    assert_eq!(authz::require_staff_case(&mut tx, &f.resident, id).await.unwrap_err().code, ErrorCode::NotFound);
}
#[tokio::test]
async fn request_info_pauses_only_pausable_clock_and_reply_extends_business_days() {
    let f = fixture(false).await;
    let id = draft(&f).await;
    cases::submission::submit_idempotent(&f.state, &f.resident, id, "submit").await.unwrap();
    let mut tx = db::write_tx(&f.state.db).await.unwrap();
    let original: Vec<(String, String)> =
        sqlx::query_as("SELECT kind,due_at FROM deadlines ORDER BY kind").fetch_all(&mut *tx).await.unwrap();
    assert_eq!(time::local_date(time::parse(&original[0].1).unwrap()).to_string(), "2026-10-13"); // weekend + Monday holiday
    assert_eq!(time::local_date(time::parse(&original[1].1).unwrap()).to_string(), "2026-10-15");
    cases::messages::post_staff_message_at(&mut tx, &f.intake, id, "Replace drawing A-101", None, true, f.state.now())
        .await
        .unwrap();
    let statuses: Vec<(String, String)> =
        sqlx::query_as("SELECT kind,status FROM deadlines ORDER BY kind").fetch_all(&mut *tx).await.unwrap();
    assert_eq!(statuses, vec![("completeness".into(), "running".into()), ("response".into(), "paused".into())]);
    tx.commit().await.unwrap();
    f.clock.set(time::parse("2026-10-14T00:00:00Z").unwrap());
    let mut tx = db::write_tx(&f.state.db).await.unwrap();
    cases::messages::applicant_reply(&mut tx, &f.state, &f.resident, id, "Drawing corrected", None).await.unwrap();
    let due: String =
        sqlx::query_scalar("SELECT due_at FROM deadlines WHERE kind='response'").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(time::local_date(time::parse(&due).unwrap()).to_string(), "2026-10-19"); // 2 business days elapsed, extended over weekend
    assert_eq!(time::to_local(time::parse(&due).unwrap()).format("%H:%M").to_string(), "17:00");
    assert_eq!(core::load_case(&mut tx, id).await.unwrap().status, "in_progress");
    let required: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM case_messages WHERE requires_response=1 AND resolved_at IS NULL")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(required, 0);
}
#[tokio::test]
async fn delayed_sweep_caps_pause_and_breach_is_notified_once() {
    let f = fixture(false).await;
    let id = draft(&f).await;
    cases::submission::submit_idempotent(&f.state, &f.resident, id, "submit").await.unwrap();
    let mut tx = db::write_tx(&f.state.db).await.unwrap();
    cases::messages::post_staff_message_at(&mut tx, &f.intake, id, "More information", None, true, f.state.now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    f.clock.set(time::parse("2026-11-01T00:00:00Z").unwrap());
    crate::deadlines::handle_job(&f.state, "deadline.sweep", &json!({})).await.unwrap();
    crate::deadlines::handle_job(&f.state, "deadline.sweep", &json!({})).await.unwrap();
    let pauses: Vec<(String, String)> =
        sqlx::query_as("SELECT ended_at,ended_reason FROM deadline_pauses").fetch_all(&f.state.db).await.unwrap();
    assert_eq!(time::local_date(time::parse(&pauses[0].0).unwrap()).to_string(), "2026-10-14");
    assert_eq!(pauses[0].1, "cap_reached");
    let breached: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM case_events WHERE kind='deadline.breached'")
        .fetch_one(&f.state.db)
        .await
        .unwrap();
    assert_eq!(breached, 2);
    let notices: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE subject='Reply clock resumed' AND channel='in_app'",
    )
    .fetch_one(&f.state.db)
    .await
    .unwrap();
    assert_eq!(notices, 1);
}
#[tokio::test]
async fn catalogue_synonyms_and_idempotent_canonical_seed() {
    let (state, _dir) = test_support::test_state().await;
    let mut tx = db::write_tx(&state.db).await.unwrap();
    super::seed(&mut tx, &state).await.unwrap();
    super::seed(&mut tx, &state).await.unwrap();
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM services").fetch_one(&mut *tx).await.unwrap(), 12);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM service_search").fetch_one(&mut *tx).await.unwrap(),
        11
    );
    for (query, expected) in [
        ("party", "rawson-hall-hire"),
        ("digger", "equipment-hire"),
        ("pothole", "road-issue"),
        ("DA", "development-application"),
        ("certificate", "planning-certificate"),
    ] {
        let query = super::catalog::prefix_query(query, true);
        let slugs: Vec<String> = sqlx::query_scalar(
            "SELECT slug FROM services WHERE id IN(SELECT service_id FROM service_search WHERE service_search MATCH ?)",
        )
        .bind(query)
        .fetch_all(&mut *tx)
        .await
        .unwrap();
        assert!(slugs.iter().any(|s| s == expected), "{expected}: {slugs:?}");
    }
    for (_, _, _, module, _, raw, _) in super::seed::catalogue() {
        let def = ServiceDefinition::parse(&raw.to_string()).unwrap();
        let issues = validation::validate_for_module(&mut tx, &def, module).await.unwrap();
        assert!(issues.iter().all(|i| i.path.starts_with("pricing.")), "{module}: {issues:?}");
    }
    let dog: String = sqlx::query_scalar(
        "SELECT v.status FROM service_versions v JOIN services s ON s.id=v.service_id WHERE s.slug='dog-registration'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(dog, "draft");
}

#[tokio::test]
async fn calendar_deadline_rolls_forward_and_late_reply_cannot_exceed_cumulative_cap() {
    let f = fixture(false).await;
    let mut raw = definition(false);
    raw["deadlines"] = json!([{"kind":"calendar_reply","label":"Calendar reply","days":1,"basis":"calendar","starts":"submitted","stops":"closed","pausable":true,"max_pause_days":5}]);
    let mut tx = db::write_tx(&f.state.db).await.unwrap();
    let v = admin::create_version(&mut tx, &f.admin, f.service, raw, None).await.unwrap();
    admin::publish(&mut tx, &f.admin, f.service, v).await.unwrap();
    tx.commit().await.unwrap();
    let id = draft(&f).await;
    cases::submission::submit_idempotent(&f.state, &f.resident, id, "submit").await.unwrap();
    let mut tx = db::write_tx(&f.state.db).await.unwrap();
    let due: String =
        sqlx::query_scalar("SELECT due_at FROM deadlines WHERE case_id=?").bind(id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(time::local_date(time::parse(&due).unwrap()).to_string(), "2026-10-13");
    cases::messages::post_staff_message_at(&mut tx, &f.intake, id, "First question", None, true, f.state.now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    f.clock.set(time::parse("2026-10-13T00:00:00Z").unwrap());
    let mut tx = db::write_tx(&f.state.db).await.unwrap();
    cases::messages::applicant_reply(&mut tx, &f.state, &f.resident, id, "First reply", None).await.unwrap();
    let due: String =
        sqlx::query_scalar("SELECT due_at FROM deadlines WHERE case_id=?").bind(id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(time::local_date(time::parse(&due).unwrap()).to_string(), "2026-10-19");
    tx.commit().await.unwrap();
    f.clock.set(time::parse("2026-10-14T00:00:00Z").unwrap());
    let mut tx = db::write_tx(&f.state.db).await.unwrap();
    cases::messages::post_staff_message_at(&mut tx, &f.intake, id, "Second question", None, true, f.state.now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    f.clock.set(time::parse("2026-10-30T00:00:00Z").unwrap());
    let mut tx = db::write_tx(&f.state.db).await.unwrap();
    cases::messages::applicant_reply(&mut tx, &f.state, &f.resident, id, "Late reply without a preceding sweep", None)
        .await
        .unwrap();
    let due: String =
        sqlx::query_scalar("SELECT due_at FROM deadlines WHERE case_id=?").bind(id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(time::local_date(time::parse(&due).unwrap()).to_string(), "2026-10-20");
    let did: i64 =
        sqlx::query_scalar("SELECT id FROM deadlines WHERE case_id=?").bind(id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(crate::deadlines::api::used_pause_days(&mut tx, did, f.state.now()).await.unwrap(), 5);
    let ended: String = sqlx::query_scalar("SELECT ended_reason FROM deadline_pauses ORDER BY id DESC LIMIT 1")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(ended, "cap_reached");
}

async fn http(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&(String, String)>,
    body: Vec<u8>,
    content_type: &str,
) -> (axum::http::StatusCode, Value) {
    use tower::ServiceExt;
    let mut request = axum::http::Request::builder().method(method).uri(path).header("content-type", content_type);
    if let Some((cookie, csrf)) = token {
        request = request.header("cookie", format!("nsh_session={cookie}")).header("X-CSRF-Token", csrf);
    }
    let response = app.clone().oneshot(request.body(axum::body::Body::from(body)).unwrap()).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 2_000_000).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}
#[tokio::test]
async fn import_creates_only_valid_drafts_and_http_projections_deny_notes_and_published_edits() {
    use axum::http::StatusCode;
    let f = fixture(false).await;
    let mut tx = db::write_tx(&f.state.db).await.unwrap();
    let admin_token = crate::auth::session::create(&mut tx, 4, true, f.state.now()).await.unwrap();
    let applicant_token = crate::auth::session::create(&mut tx, 1, true, f.state.now()).await.unwrap();
    tx.commit().await.unwrap();
    let app = crate::app::build_router(f.state.clone());
    let (status, public) = http(&app, "GET", "/api/public/services", None, vec![], "application/json").await;
    assert_eq!(status, StatusCode::OK, "{public}");
    assert_eq!(public["items"].as_array().unwrap().len(), 1);
    let sample = include_str!("../../seed-data/service-import-sample.json");
    let bytes=format!("--test-boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"services.json\"\r\nContent-Type: application/json\r\n\r\n{sample}\r\n--test-boundary--\r\n").into_bytes();
    let (status, report) = http(
        &app,
        "POST",
        "/api/admin/service-imports",
        Some(&admin_token),
        bytes,
        "multipart/form-data; boundary=test-boundary",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["report"]["items"].as_array().unwrap().iter().filter(|r| r["valid"] == true).count(), 2);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM services").fetch_one(&f.state.db).await.unwrap(),
        1,
        "validation creates nothing"
    );
    let path = format!("/api/admin/service-imports/{}/apply", report["id"]);
    let (status, result) = http(&app, "POST", &path, Some(&admin_token), b"{}".to_vec(), "application/json").await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let (_, retry) = http(&app, "POST", &path, Some(&admin_token), b"{}".to_vec(), "application/json").await;
    assert_eq!(result, retry);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM service_versions WHERE status='draft'")
            .fetch_one(&f.state.db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM service_versions WHERE status='published'")
            .fetch_one(&f.state.db)
            .await
            .unwrap(),
        1
    );
    let path = format!("/api/admin/services/{}/versions/{}", f.service, f.version);
    let (status, _) =
        http(&app, "PUT", &path, Some(&admin_token), definition(false).to_string().into_bytes(), "application/json")
            .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let id = draft(&f).await;
    cases::submission::submit_idempotent(&f.state, &f.resident, id, "submit").await.unwrap();
    let (status, _) =
        http(&app, "GET", &format!("/api/cases/{id}/notes"), Some(&applicant_token), vec![], "application/json").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, detail) =
        http(&app, "GET", &format!("/api/cases/{id}"), Some(&applicant_token), vec![], "application/json").await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["case"]["confidential"], false);
    assert!(detail.get("assignments").is_none());
}
#[tokio::test]
async fn ai_disabled_is_404_without_affecting_catalogue() {
    let (state, _dir) = test_support::test_state().await;
    let mut config = (*state.cfg).clone();
    config.ai_enabled = false;
    let state = AppState { cfg: std::sync::Arc::new(config), ..state };
    let app = crate::app::build_router(state);
    let (status, _) =
        http(&app, "POST", "/mock/ai/suggest", None, b"{\"text\":\"Name: ___\"}".to_vec(), "application/json").await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
    let (status, _) = http(&app, "GET", "/api/public/services", None, vec![], "application/json").await;
    assert_eq!(status, axum::http::StatusCode::OK);
}
