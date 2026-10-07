use super::*;
use crate::{
    auth::{Actor, RoleGrant, UserKind},
    authz::{self, Role},
    cases::core::{self, NewCase, Visibility},
    db::write_tx,
};
const NOW: &str = "2026-10-07T00:00:00.000Z";
async fn fixture() -> (AppState, tempfile::TempDir) {
    let (state, dir) = crate::state::test_support::test_state().await;
    let mut tx = write_tx(&state.db).await.unwrap();
    seed(&mut tx, &state).await.unwrap();
    seed(&mut tx, &state).await.unwrap();
    for (id, kind, name) in [
        (1, "resident", "Alexey"),
        (2, "staff", "Olga"),
        (3, "staff", "Ruth"),
        (4, "staff", "Helen"),
        (5, "resident", "Ben"),
    ] {
        sqlx::query("INSERT INTO users(id,email,display_name,kind,created_at) VALUES(?,?,?,?,?)")
            .bind(id)
            .bind(format!("u{id}@example.invalid"))
            .bind(name)
            .bind(kind)
            .bind(NOW)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    for (sid, module) in [(1, "generic"), (2, "complaint")] {
        sqlx::query(
            "INSERT INTO services(id,slug,name,category,module,department,created_at) VALUES(?,?,?,'Test',?,'Test',?)",
        )
        .bind(sid)
        .bind(format!("s{sid}"))
        .bind(format!("Service {sid}"))
        .bind(module)
        .bind(NOW)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("INSERT INTO service_versions(id,service_id,version,status,definition_json,created_at) VALUES(?,?,1,'published','{}',?)").bind(sid).bind(sid).bind(NOW).execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    (state, dir)
}
fn actor(id: i64, roles: &[Role]) -> Actor {
    Actor {
        user_id: id,
        kind: if id == 1 || id == 5 { UserKind::Resident } else { UserKind::Staff },
        roles: roles.iter().map(|r| RoleGrant { role: *r, scope_service_id: None }).collect(),
        display_name: format!("User {id}"),
        mfa_passed: true,
    }
}
async fn new_case(tx: &mut SqliteConnection, module: &str, status: &str) -> core::CaseRow {
    let sid = if module == "complaint" { 2 } else { 1 };
    core::create_case(
        tx,
        NewCase {
            service_id: sid,
            service_version_id: sid,
            module: module.into(),
            title: "Fictional request".into(),
            status: status.into(),
            applicant_user_id: Some(1),
            applicant_org_id: None,
            applicant_name: "Alexey".into(),
            applicant_email: None,
            applicant_phone: None,
            intake_channel: "online".into(),
            recorded_by_user_id: None,
            property_ref: None,
        },
    )
    .await
    .unwrap()
}
#[tokio::test]
async fn every_metric_count_matches_drilldown_and_denials() {
    let (state, _dir) = fixture().await;
    let mut tx = write_tx(&state.db).await.unwrap();
    for status in ["submitted", "in_progress", "waiting_on_applicant", "completed", "refused", "withdrawn", "cancelled"]
    {
        let case = new_case(&mut tx, "generic", status).await;
        sqlx::query("UPDATE cases SET submitted_at=?,closed_at=CASE WHEN status IN ('completed','refused','withdrawn','cancelled') THEN ? END WHERE id=?").bind(NOW).bind(NOW).bind(case.id).execute(&mut *tx).await.unwrap();
        if status == "in_progress" {
            sqlx::query("UPDATE cases SET reopened_count=1 WHERE id=?").bind(case.id).execute(&mut *tx).await.unwrap();
        }
        if status == "submitted" {
            sqlx::query("INSERT INTO deadlines(case_id,kind,label,basis,duration_days,pausable,started_at,due_at,status,policy_json) VALUES(?,'response','Response','business',3,0,?,'2026-01-01T00:00:00.000Z','breached','{}')").bind(case.id).bind(NOW).execute(&mut *tx).await.unwrap();
        }
    }
    let secret = new_case(&mut tx, "complaint", "completed").await;
    sqlx::query("UPDATE cases SET closed_at=?,submitted_at=? WHERE id=?")
        .bind(NOW)
        .bind(NOW)
        .bind(secret.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO case_access_denials(case_id,user_id,reason,created_at) VALUES(?,4,'Subject of complaint',?)",
    )
    .bind(secret.id)
    .bind(NOW)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let mut conn = state.db.acquire().await.unwrap();
    let manager = actor(4, &[Role::Manager]);
    let f = metrics::Filters { from: Some("2026-01-01".into()), to: Some("2026-12-31".into()), ..Default::default() };
    for metric in metrics::METRICS {
        let count = metrics::count(&mut conn, &state, &manager, metric, &f).await.unwrap();
        let list = metrics::cases(&mut conn, &state, &manager, metric, &f).await.unwrap();
        assert_eq!(count as usize, list.len(), "{metric}");
        assert!(list.iter().all(|r| r["id"] != secret.id));
    }
    assert_eq!(metrics::count(&mut conn, &state, &manager, "completed", &f).await.unwrap(), 1);
    assert_eq!(metrics::count(&mut conn, &state, &manager, "cancelled", &f).await.unwrap(), 1);
    assert_eq!(metrics::count(&mut conn, &state, &manager, "reopened", &f).await.unwrap(), 1);
}
#[tokio::test]
async fn subject_endpoint_excludes_even_managers_from_read_search_count_and_export() {
    let (state, _dir) = fixture().await;
    let mut tx = write_tx(&state.db).await.unwrap();
    let case = new_case(&mut tx, "complaint", "completed").await;
    crate::auth::users::grant_role(&mut tx, 4, Role::Manager, None, None).await.unwrap();
    sqlx::query("UPDATE cases SET submitted_at=?,closed_at=? WHERE id=?")
        .bind(NOW)
        .bind(NOW)
        .bind(case.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    complaints::subjects(
        axum::extract::State(state.clone()),
        actor(3, &[Role::ComplaintsOfficer]),
        crate::web::Path(case.id),
        crate::web::Json(complaints::Subjects { staff_user_ids: vec![2, 4], expected_revision: Some(1) }),
    )
    .await
    .unwrap();
    let mut conn = state.db.acquire().await.unwrap();
    for denied in [actor(2, &[Role::Intake]), actor(4, &[Role::Manager])] {
        assert_eq!(
            authz::require_case(&mut conn, &denied, case.id).await.unwrap_err().code,
            crate::error::ErrorCode::NotFound
        );
        let scope = authz::case_scope_sql(&denied);
        let ids: Vec<i64> = crate::db::bind_all_scalar(
            sqlx::query_scalar(&format!("SELECT c.id FROM cases c WHERE {}", scope.sql)),
            &scope.binds,
        )
        .fetch_all(&mut *conn)
        .await
        .unwrap();
        assert!(!ids.contains(&case.id));
        assert_eq!(
            export::export_case(&state, &denied, case.id).await.unwrap_err().code,
            crate::error::ErrorCode::NotFound
        );
    }
    assert!(authz::require_staff_case(&mut conn, &actor(3, &[Role::ComplaintsOfficer]), case.id).await.is_ok());
    drop(conn);
    let manager = session(&state, 4).await;
    let (status, results) = request(&state, &manager, "GET", "/api/records/search?person=Alexey", Value::Null).await;
    assert_eq!(status, 200);
    assert!(results.as_array().unwrap().is_empty());
    let (status, results) =
        request(&state, &manager, "GET", "/api/staff/dashboard?from=2026-01-01&to=2026-12-31", Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(results["metrics"]["received"], 0);
    assert_eq!(results["metrics"]["completed"], 0);
}
#[tokio::test]
async fn outbox_outage_and_lost_response_are_recoverable_without_duplicates() {
    let (state, _dir) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut cfg = (*state.cfg).clone();
    cfg.internal_base_url = format!("http://{}", listener.local_addr().unwrap());
    let state = AppState { cfg: std::sync::Arc::new(cfg), ..state };
    let app = crate::mock::records::routes().with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut tx = write_tx(&state.db).await.unwrap();
    crate::auth::users::grant_role(&mut tx, 2, Role::Sysadmin, None, None).await.unwrap();
    integrations::enqueue(&mut tx, "content_manager", None, "record.case_closed", 100, json!({"fictional":true}))
        .await
        .unwrap();
    integrations::enqueue(&mut tx, "content_manager", None, "record.case_closed", 100, json!({"fictional":true}))
        .await
        .unwrap();
    sqlx::query("UPDATE mock_system_state SET outage=1 WHERE system_code='content_manager'")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let id: i64 = sqlx::query_scalar("SELECT id FROM integration_deliveries").fetch_one(&state.db).await.unwrap();
    assert!(integrations::deliver(&state, id).await.is_err());
    let status: String = sqlx::query_scalar("SELECT status FROM integration_deliveries WHERE id=?")
        .bind(id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(status, "failed");
    let admin = session(&state, 2).await;
    let (status, response) =
        request(&state, &admin, "POST", &format!("/api/admin/integrations/{id}/retry"), json!({})).await;
    assert_eq!(status, 200, "{response}");

    sqlx::query("UPDATE mock_system_state SET outage=0,drop_responses=1 WHERE system_code='content_manager'")
        .execute(&state.db)
        .await
        .unwrap();
    assert!(integrations::deliver(&state, id).await.is_err()); // remote stores it; five-second client timeout
    integrations::deliver(&state, id).await.unwrap();
    integrations::deliver(&state, id).await.unwrap();
    let (status, reference): (String, String) =
        sqlx::query_as("SELECT status,external_ref FROM integration_deliveries WHERE id=?")
            .bind(id)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(status, "accepted");
    assert!(reference.starts_with("CM-"));
    let remote: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mock_external_records").fetch_one(&state.db).await.unwrap();
    assert_eq!(remote, 1);
    let sender: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM integration_deliveries").fetch_one(&state.db).await.unwrap();
    assert_eq!(sender, 1);
    server.abort();
}
#[tokio::test]
async fn backup_roundtrip_and_tamper_detection() {
    let (state, dir) = fixture().await;
    let bytes = crate::pdf::simple_document("Test", &[], &[("Demo", "Fictional".into())]);
    let blob = crate::storage::put(&state, &bytes, "test.pdf", crate::storage::AllowList::Docs, None).await.unwrap();
    let mut tx = write_tx(&state.db).await.unwrap();
    let case = new_case(&mut tx, "generic", "completed").await;
    let doc:i64=sqlx::query_scalar("INSERT INTO documents(case_id,category,title,visibility,created_at) VALUES(?,'application','Fictional test document','applicant',?) RETURNING id").bind(case.id).bind(NOW).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO document_versions(document_id,version,blob_id,uploaded_at) VALUES(?,1,?,?)")
        .bind(doc)
        .bind(blob.id)
        .bind(NOW)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let destination = dir.path().join("backup");
    backup::backup(&state, &destination).await.unwrap();
    backup::restore_check(&state, &destination).await.unwrap();
    std::fs::write(crate::storage::blob_path(&destination.join("blobs"), &blob.sha256), b"tampered").unwrap();
    assert!(backup::restore_check(&state, &destination).await.is_err());
    let status: String = sqlx::query_scalar("SELECT status FROM backup_runs ORDER BY id DESC LIMIT 1")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(status, "failed");
}

async fn session(state: &AppState, user: i64) -> (String, String) {
    let mut tx = write_tx(&state.db).await.unwrap();
    let pair = crate::auth::session::create(&mut tx, user, true, state.now()).await.unwrap();
    tx.commit().await.unwrap();
    pair
}
async fn request(
    state: &AppState,
    credentials: &(String, String),
    method: &str,
    path: &str,
    body: Value,
) -> (u16, Value) {
    use tower::ServiceExt;
    let app = routes()
        .merge(crate::documents::routes())
        .layer(axum::middleware::from_fn_with_state(state.clone(), crate::auth::extract::session_middleware))
        .with_state(state.clone());
    let req = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("Cookie", format!("nsh_session={}", credentials.0))
        .header("X-CSRF-Token", &credentials.1)
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let bytes = axum::body::to_bytes(res.into_body(), 2 * 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}
#[tokio::test]
async fn invitations_bind_email_and_revocation_removes_organisation_case_access() {
    let (state, _dir) = fixture().await;
    let mut tx = write_tx(&state.db).await.unwrap();
    sqlx::query("INSERT INTO organisations(id,name,created_at) VALUES(1,'Fictional Island Builders',?)")
        .bind(NOW)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships(organisation_id,user_id,invite_email,role,status,created_at) VALUES(1,1,'u1@example.invalid','owner','active',?)").bind(NOW).execute(&mut *tx).await.unwrap();
    let case = new_case(&mut tx, "generic", "submitted").await;
    sqlx::query("UPDATE cases SET applicant_org_id=1,applicant_user_id=5 WHERE id=?")
        .bind(case.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let (_, version) = crate::documents::api::attach_generated(
        &mut tx,
        &state,
        case.id,
        "application",
        "Organisation evidence",
        Visibility::Applicant,
        crate::pdf::simple_document("Organisation evidence", &[], &[]),
        Some(1),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let owner = session(&state, 1).await;
    let member = session(&state, 5).await;
    let (status, invitation) =
        request(&state, &owner, "POST", "/api/my/organisations/1/invites", json!({"email":"u5@example.invalid"})).await;
    assert_eq!(status, 200, "{invitation}");
    let link: String =
        sqlx::query_scalar("SELECT link FROM notifications WHERE channel='email' ORDER BY id DESC LIMIT 1")
            .fetch_one(&state.db)
            .await
            .unwrap();
    let token = link.rsplit('/').next().unwrap();
    let path = format!("/api/my/invites/{token}/accept");
    assert_eq!(request(&state, &owner, "POST", &path, json!({})).await.0, 404);
    assert_eq!(request(&state, &member, "POST", &path, json!({})).await.0, 200);
    let mut conn = state.db.acquire().await.unwrap();
    assert_eq!(authz::case_access(&mut conn, &actor(5, &[]), case.id).await.unwrap(), authz::CaseAccess::Applicant);
    drop(conn);
    let download = format!("/api/document-versions/{version}/download");
    assert_eq!(request(&state, &member, "GET", &download, json!({})).await.0, 200);
    let mid = invitation["id"].as_i64().unwrap();
    assert_eq!(
        request(&state, &owner, "POST", &format!("/api/my/organisations/1/members/{mid}/revoke"), json!({})).await.0,
        200
    );
    assert_eq!(request(&state, &member, "GET", &download, json!({})).await.0, 404);
    assert_eq!(request(&state, &owner, "GET", &download, json!({})).await.0, 200);
    let mut conn = state.db.acquire().await.unwrap();
    assert_eq!(
        authz::require_case(&mut conn, &actor(5, &[]), case.id).await.unwrap_err().code,
        crate::error::ErrorCode::NotFound
    );
    drop(conn);
    assert_eq!(
        request(&state, &member, "POST", &path, json!({})).await.0,
        404,
        "Revoked invitation must not restore access"
    );
}
#[tokio::test]
async fn legal_hold_blocks_disposal_and_only_manager_can_grant_authority() {
    let (state, _dir) = fixture().await;
    let mut tx = write_tx(&state.db).await.unwrap();
    crate::auth::users::grant_role(&mut tx, 4, Role::Manager, None, None).await.unwrap();
    crate::auth::users::grant_role(&mut tx, 2, Role::Sysadmin, None, None).await.unwrap();
    let case = new_case(&mut tx, "generic", "completed").await;
    sqlx::query("UPDATE cases SET closed_at=?,retention_until='2020-01-01' WHERE id=?")
        .bind(NOW)
        .bind(case.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let manager = session(&state, 4).await;
    let admin = session(&state, 2).await;
    let path = format!("/api/records/cases/{}/legal-hold", case.id);
    assert_eq!(request(&state, &manager, "POST", &path, json!({"reason":"Fictional dispute"})).await.0, 200);
    let (status, error) = request(
        &state,
        &manager,
        "POST",
        &format!("/api/records/cases/{}/dispose", case.id),
        json!({"reason":"Retention elapsed"}),
    )
    .await;
    assert_eq!(status, 409);
    assert!(error["error"]["message"].as_str().unwrap().contains("legal hold"));
    assert_eq!(
        request(
            &state,
            &admin,
            "POST",
            "/api/staff/decision-authorities",
            json!({"user_id":3,"decision_type":"planning_certificate"})
        )
        .await
        .0,
        403
    );
    assert_eq!(
        request(
            &state,
            &manager,
            "POST",
            "/api/staff/decision-authorities",
            json!({"user_id":4,"decision_type":"planning_certificate"})
        )
        .await
        .0,
        403
    );
    assert_eq!(
        request(
            &state,
            &manager,
            "POST",
            "/api/staff/decision-authorities",
            json!({"user_id":3,"decision_type":"planning_certificate"})
        )
        .await
        .0,
        200
    );
}

#[tokio::test]
async fn legacy_preview_reports_bad_rows_and_both_duplicate_kinds() {
    let (state, _dir) = fixture().await;
    let mut tx = write_tx(&state.db).await.unwrap();
    crate::auth::users::grant_role(&mut tx, 2, Role::Sysadmin, None, None).await.unwrap();
    for (i, slug) in [
        "rawson-hall-hire",
        "planning-certificate",
        "development-application",
        "equipment-hire",
        "driveway-crossover",
        "road-issue",
        "complaint",
        "council-record-copy",
        "modify-approval",
    ]
    .iter()
    .enumerate()
    {
        let sid = 10 + i as i64;
        sqlx::query("INSERT INTO services(id,slug,name,category,module,department,created_at) VALUES(?,?,?,'Demo','generic','Customer Care',?)").bind(sid).bind(slug).bind(slug).bind(NOW).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO service_versions(id,service_id,version,status,definition_json,created_at) VALUES(?,?,1,'published','{}',?)").bind(sid).bind(sid).bind(NOW).execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    let admin = session(&state, 2).await;
    let csv = include_str!("../../seed-data/legacy/legacy-cases-sample.csv");
    let (status, report) =
        request(&state, &admin, "POST", "/api/admin/legacy-imports", json!({"filename":"sample.csv","csv":csv})).await;
    assert_eq!(status, 200, "{report}");
    assert_eq!(report["rows"].as_array().unwrap().len(), 15);
    assert_eq!(report["errors"], 1);
    assert_eq!(report["duplicates"], 1);
    assert_eq!(report["possible_duplicates"], 1);
    let batch = report["id"].as_i64().unwrap();
    let (status, again) =
        request(&state, &admin, "GET", &format!("/api/admin/legacy-imports/{batch}"), Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(again, report);
}

#[tokio::test]
async fn disposal_endpoint_preserves_decision_evidence_and_shared_blob_until_last_consumer() {
    let (state, _dir) = fixture().await;
    let mut tx = write_tx(&state.db).await.unwrap();
    crate::auth::users::grant_role(&mut tx, 4, Role::Manager, None, None).await.unwrap();
    let a = new_case(&mut tx, "generic", "completed").await;
    let b = new_case(&mut tx, "generic", "completed").await;
    let bytes = crate::pdf::simple_document("Retention endpoint test", &[], &[]);
    let mut versions = Vec::new();
    for c in [&a, &b] {
        crate::cases::core::set_import_dates(&mut tx, c.id, "2000-01-01T00:00:00Z", Some("2001-01-01T00:00:00Z"))
            .await
            .unwrap();
        sqlx::query("UPDATE cases SET retention_until='2008-01-01' WHERE id=?")
            .bind(c.id)
            .execute(&mut *tx)
            .await
            .unwrap();
        versions.push(
            crate::documents::api::attach_generated(
                &mut tx,
                &state,
                c.id,
                "application",
                "Retained drawing",
                Visibility::Applicant,
                bytes.clone(),
                Some(1),
            )
            .await
            .unwrap()
            .1,
        );
    }
    let decision: i64 = sqlx::query_scalar("INSERT INTO decisions(case_id,decision_type,outcome,reasons,status,prepared_by,created_at) VALUES(?,'building_approval','approved','Fictional evidence','draft',4,?) RETURNING id")
        .bind(a.id).bind(NOW).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO decision_evidence(decision_id,document_version_id) VALUES(?,?)")
        .bind(decision)
        .bind(versions[0])
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE decisions SET status='issued' WHERE id=?").bind(decision).execute(&mut *tx).await.unwrap();
    let (blob, hash): (i64, String) =
        sqlx::query_as("SELECT b.id,b.sha256 FROM blobs b JOIN document_versions v ON v.blob_id=b.id WHERE v.id=?")
            .bind(versions[0])
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    // Age the fixture beyond the staging grace so endpoint-triggered GC actually deletes bytes.
    std::fs::File::open(crate::storage::blob_path(&state.cfg.blobs_dir(), &hash))
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(std::time::SystemTime::UNIX_EPOCH))
        .unwrap();
    let manager = session(&state, 4).await;
    for (index, c) in [&a, &b].into_iter().enumerate() {
        let path = format!("/api/records/cases/{}/dispose", c.id);
        let (status, body) = request(
            &state,
            &manager,
            "POST",
            &path,
            json!({"reason":"Retention elapsed", "expected_revision":c.revision}),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            request(
                &state,
                &manager,
                "GET",
                &format!("/api/document-versions/{}/download", versions[index]),
                json!({})
            )
            .await
            .0,
            404
        );
        if index == 0 {
            assert_eq!(crate::storage::read(&state, blob).await.unwrap().1, bytes);
        }
    }
    assert_eq!(crate::storage::read(&state, blob).await.unwrap_err().code, crate::error::ErrorCode::NotFound);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT document_version_id FROM decision_evidence WHERE decision_id=?")
            .bind(decision)
            .fetch_one(&state.db)
            .await
            .unwrap(),
        versions[0]
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM document_versions WHERE id IN (?,?)")
            .bind(versions[0])
            .bind(versions[1])
            .fetch_one(&state.db)
            .await
            .unwrap(),
        2
    );
}
