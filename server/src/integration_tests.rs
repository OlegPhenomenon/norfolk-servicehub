//! Invariants at module boundaries; the HTTP journeys live in scripts/smoke-integration.sh.
use crate::{
    auth::{Actor, session, users},
    cases::{
        self,
        core::{self, CaseRow, Visibility},
    },
    db::write_tx,
    documents,
    error::ErrorCode,
    jobs,
    notify::{self, Notice},
    state::{AppState, test_support},
    storage, time,
};
use serde_json::json;
use std::time::Duration;

async fn fixture() -> (AppState, tempfile::TempDir) {
    let (state, dir) = test_support::test_state().await;
    crate::seed::seed_base(&state).await.unwrap();
    (state, dir)
}
async fn persona(tx: &mut sqlx::SqliteConnection, key: &str) -> Actor {
    let id =
        sqlx::query_scalar("SELECT id FROM users WHERE persona_key=?").bind(key).fetch_one(&mut *tx).await.unwrap();
    Actor::load(tx, id, true).await.unwrap()
}
async fn draft(tx: &mut sqlx::SqliteConnection, actor: &Actor) -> CaseRow {
    cases::drafts::create_draft(tx, actor, "complaint", Default::default()).await.unwrap()
}

#[tokio::test]
async fn deactivation_revokes_all_sessions_atomically_and_unseeded_scheduler_queues_backup() {
    let (state, _dir) = fixture().await;
    let mut tx = write_tx(&state.db).await.unwrap();
    let actor = persona(&mut tx, "alexey").await;
    let first = session::create(&mut tx, actor.user_id, true, state.now()).await.unwrap().0;
    let second = session::create(&mut tx, actor.user_id, true, state.now()).await.unwrap().0;
    tx.commit().await.unwrap();
    let mut tx = write_tx(&state.db).await.unwrap();
    users::deactivate_user(&mut tx, actor.user_id).await.unwrap();
    tx.rollback().await.unwrap();
    assert!(session::resolve(&state, &first).await.unwrap().is_some());
    let mut tx = write_tx(&state.db).await.unwrap();
    users::deactivate_user(&mut tx, actor.user_id).await.unwrap();
    tx.commit().await.unwrap();
    for token in [first, second] {
        assert!(session::resolve(&state, &token).await.unwrap().is_none());
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sessions WHERE user_id=?")
            .bind(actor.user_id)
            .fetch_one(&state.db)
            .await
            .unwrap(),
        0
    );
    let (unseeded, _dir) = test_support::test_state().await;
    jobs::schedule_tick(&unseeded).await.unwrap();
    jobs::schedule_tick(&unseeded).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM jobs WHERE kind='records.backup'")
            .fetch_one(&unseeded.db)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn confidential_submission_review_and_owner_replacement_preserve_frozen_evidence() {
    let (state, _dir) = fixture().await;
    let mut tx = write_tx(&state.db).await.unwrap();
    let resident = persona(&mut tx, "alexey").await;
    let ruth = persona(&mut tx, "ruth").await;
    let helen = persona(&mut tx, "helen").await;
    let original = draft(&mut tx, &resident).await;
    cases::drafts::save_answers(&mut tx, &resident, original.id, &json!({"applicant_name":"Alexey", "postal_address":"Fictional address", "complaint":"Sensitive complaint details", "desired_outcome":"Review", "declaration":true})).await.unwrap();
    let original = cases::submission::submit_case(&mut tx, &state, &resident, original.id).await.unwrap();
    assert_eq!(original.current_step.as_deref(), Some("triage"));
    notify::send(
        &mut tx,
        Notice {
            user_id: Some(resident.user_id),
            email: Some("private@example.invalid".into()),
            phone: Some("+12345".into()),
            case_id: Some(original.id),
            subject: "Secret complaint subject".into(),
            body: "Sensitive complaint details".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let notifications: Vec<(String, String)> = sqlx::query_as("SELECT subject,body FROM notifications WHERE case_id=?")
        .bind(original.id)
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert!(notifications.iter().all(|(subject, body)| !subject.contains("Secret")
        && !body.contains("Sensitive")
        && body.contains("sign in to read it")));
    cases::api::assign_owner(&mut tx, &ruth, original.id, helen.user_id, "Transfer ownership").await.unwrap();
    let owners: Vec<i64> = sqlx::query_scalar(
        "SELECT user_id FROM case_assignments WHERE case_id=? AND role='owner' AND ended_at IS NULL",
    )
    .bind(original.id)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert_eq!(owners, [helen.user_id]);
    cases::workflow::close(&mut tx, &state, &ruth, original.id, "completed", "Response completed").await.unwrap();
    let original = core::load_case(&mut tx, original.id).await.unwrap();
    let review = cases::api::create_review(&mut tx, &resident, &original, "Independent review").await.unwrap();
    assert_ne!(review.number, original.number);
    assert_eq!(review.current_step.as_deref(), Some("triage"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM case_assignments WHERE case_id=?")
            .bind(review.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        0,
        "copy exclusions before assignment"
    );
    let snapshots: Vec<(String, String)> = sqlx::query_as(
        "SELECT definition_snapshot_json,answers_json FROM submissions WHERE case_id IN (?,?) ORDER BY case_id",
    )
    .bind(original.id)
    .bind(review.id)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert_eq!(snapshots[0], snapshots[1]);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM case_links WHERE from_case_id=? AND to_case_id=? AND kind='review_of'"
        )
        .bind(review.id)
        .bind(original.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap(),
        1
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn disposal_preserves_metadata_shared_bytes_and_non_document_consumers() {
    let (state, _dir) = fixture().await;
    let mut tx = write_tx(&state.db).await.unwrap();
    let resident = persona(&mut tx, "alexey").await;
    let a = draft(&mut tx, &resident).await;
    let b = draft(&mut tx, &resident).await;
    let bytes = crate::pdf::simple_document("Shared retention test", &[], &[]);
    let mut versions = Vec::new();
    for case in [&a, &b] {
        let (_, version) = documents::api::attach_generated(
            &mut tx,
            &state,
            case.id,
            "application",
            "Drawing",
            Visibility::Applicant,
            bytes.clone(),
            resident.db_id(),
        )
        .await
        .unwrap();
        versions.push(version);
    }
    let (blob, hash): (i64, String) =
        sqlx::query_as("SELECT b.id,b.sha256 FROM blobs b JOIN document_versions v ON v.blob_id=b.id WHERE v.id=?")
            .bind(versions[0])
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    core::set_import_dates(&mut tx, a.id, "2000-01-01T00:00:00Z", Some("2001-01-01T00:00:00Z")).await.unwrap();
    let imported = core::load_case(&mut tx, a.id).await.unwrap();
    assert_eq!(imported.created_at, "2000-01-01T00:00:00Z");
    assert_eq!(imported.closed_at.as_deref(), Some("2001-01-01T00:00:00Z"));
    assert!(documents::api::dispose_case_files(&mut tx, a.id).await.unwrap().is_empty());
    tx.commit().await.unwrap();
    storage::gc_older_than(&state, Duration::ZERO).await.unwrap();
    assert_eq!(storage::read(&state, blob).await.unwrap().1, bytes);
    let mut tx = write_tx(&state.db).await.unwrap();
    // A draft service's source form also consumes this hash, independently of document versions.
    let source: i64 = sqlx::query_scalar("INSERT INTO service_versions(service_id,version,status,definition_json,source_blob_id,created_at) VALUES(?,99,'draft','{}',?,?) RETURNING id")
        .bind(a.service_id).bind(blob).bind(time::now_str()).fetch_one(&mut *tx).await.unwrap();
    assert!(documents::api::dispose_case_files(&mut tx, b.id).await.unwrap().is_empty());
    tx.commit().await.unwrap();
    storage::gc_older_than(&state, Duration::ZERO).await.unwrap();
    assert_eq!(storage::read(&state, blob).await.unwrap().1, bytes);
    let mut tx = write_tx(&state.db).await.unwrap();
    sqlx::query("UPDATE service_versions SET source_blob_id=NULL WHERE id=?")
        .bind(source)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(storage::disposed_orphan_hashes(&mut tx).await.unwrap(), std::slice::from_ref(&hash));
    tx.commit().await.unwrap();
    // Restaging an old hash protects the upload's uncommitted registration during the GC grace.
    let staged = storage::stage(&state, &bytes, "drawing.pdf", storage::AllowList::Docs).await.unwrap();
    assert_eq!(storage::gc(&state).await.unwrap(), 0);
    // Simulate staging that outlived the grace period: registration must restore bytes under
    // the writer lock, rather than committing a dangling reference after GC.
    assert_eq!(storage::gc_older_than(&state, Duration::ZERO).await.unwrap(), 1);
    let mut tx = write_tx(&state.db).await.unwrap();
    let c = draft(&mut tx, &resident).await;
    let registered = storage::register(&mut tx, staged, resident.db_id()).await.unwrap();
    assert_eq!(std::fs::read(storage::blob_path(&state.cfg.blobs_dir(), &registered.sha256)).unwrap(), bytes);
    let (_, extra) = documents::api::insert(
        &mut tx,
        c.id,
        "application",
        "Restaged",
        Visibility::Applicant,
        None,
        registered.id,
        resident.db_id(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(storage::gc_older_than(&state, Duration::ZERO).await.unwrap(), 0);
    let mut tx = write_tx(&state.db).await.unwrap();
    assert_eq!(documents::api::dispose_case_files(&mut tx, c.id).await.unwrap(), [hash]);
    tx.commit().await.unwrap();
    assert_eq!(storage::gc_older_than(&state, Duration::ZERO).await.unwrap(), 1);
    assert_eq!(storage::read(&state, blob).await.unwrap_err().code, ErrorCode::NotFound);
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM document_versions WHERE id IN (?,?,?) ORDER BY id")
        .bind(versions[0])
        .bind(versions[1])
        .bind(extra)
        .fetch_all(&state.db)
        .await
        .unwrap();
    assert_eq!(ids, [versions[0], versions[1], extra]);
    assert!(sqlx::query("PRAGMA foreign_key_check").fetch_all(&state.db).await.unwrap().is_empty());
}

#[tokio::test]
async fn replacement_resolution_does_not_resume_an_unrelated_information_request() {
    let (state, _dir) = fixture().await;
    let mut tx = write_tx(&state.db).await.unwrap();
    let resident = persona(&mut tx, "alexey").await;
    let ruth = persona(&mut tx, "ruth").await;
    let c = draft(&mut tx, &resident).await;
    cases::drafts::save_answers(&mut tx, &resident, c.id, &json!({"applicant_name":"Alexey", "postal_address":"Fictional address", "complaint":"Feedback", "desired_outcome":"Review", "declaration":true})).await.unwrap();
    cases::submission::submit_case(&mut tx, &state, &resident, c.id).await.unwrap();
    let (_, version) = documents::api::attach_generated(
        &mut tx,
        &state,
        c.id,
        "application",
        "Evidence",
        Visibility::Applicant,
        crate::pdf::simple_document("Evidence", &[], &[]),
        resident.db_id(),
    )
    .await
    .unwrap();
    let mid = cases::messages::post_staff_message_at(
        &mut tx,
        &ruth,
        c.id,
        "Replace evidence",
        Some(version),
        true,
        state.now(),
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO document_comments(document_version_id,author_user_id,visibility,body,created_at,request_new_version,message_id,resolved_at,resolved_by_version_id) VALUES(?,?,'applicant','Replace evidence',?,1,?,?,?)")
        .bind(version).bind(ruth.user_id).bind(time::now_str()).bind(mid).bind(time::now_str()).bind(version).execute(&mut *tx).await.unwrap();
    let other: i64 = sqlx::query_scalar("INSERT INTO case_messages(case_id,author_user_id,from_staff,body,requires_response,created_at) VALUES(?,?,1,'Unrelated information',1,?) RETURNING id")
        .bind(c.id).bind(ruth.user_id).bind(time::now_str()).fetch_one(&mut *tx).await.unwrap();
    cases::messages::resolve_document_requests(&mut tx, &state, c.id).await.unwrap();
    assert_eq!(core::load_case(&mut tx, c.id).await.unwrap().status, "waiting_on_applicant");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM deadlines WHERE case_id=? AND status='paused'")
            .bind(c.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        1
    );
    assert!(
        sqlx::query_scalar::<_, Option<String>>("SELECT resolved_at FROM case_messages WHERE id=?")
            .bind(other)
            .fetch_one(&mut *tx)
            .await
            .unwrap()
            .is_none()
    );
    cases::messages::applicant_reply(&mut tx, &state, &resident, c.id, "Here is the unrelated information", None)
        .await
        .unwrap();
    assert_eq!(core::load_case(&mut tx, c.id).await.unwrap().status, "in_progress");
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn seeded_module_handlers_and_task_kinds_reach_their_owner_implementations() {
    let (state, _dir) = fixture().await;
    let mut tx = write_tx(&state.db).await.unwrap();
    let resident = persona(&mut tx, "alexey").await;
    let definitions: Vec<(String, String)> = sqlx::query_as("SELECT s.slug,v.definition_json FROM services s JOIN service_versions v ON v.service_id=s.id WHERE v.status='published'")
        .fetch_all(&mut *tx).await.unwrap();
    let start = time::fmt(state.now() + chrono::Duration::days(30));
    let end = time::fmt(state.now() + chrono::Duration::days(30) + chrono::Duration::hours(3));
    let mut tasks = std::collections::BTreeSet::new();
    let mut handlers = std::collections::BTreeSet::new();
    for (slug, snapshot) in definitions {
        let c = cases::drafts::create_draft(&mut tx, &resident, &slug, Default::default()).await.unwrap();
        let def = crate::services::definition::load_for_case(&mut tx, &c).await.unwrap();
        if slug == "rawson-hall-hire" {
            assert!(def.field("event_name").is_some() && def.field("setup_notes").is_some());
            for condition in ["10 pm", "noon", "$20M", "seven days", "thirty days"] {
                assert!(def.summary.contains(condition));
            }
        }
        if slug == "modify-approval" {
            assert_eq!(
                def.field("original_approval").unwrap().field_type,
                crate::services::definition::FieldType::DecisionRef
            );
        }
        if slug.ends_with("notice") {
            assert!(def.field("project_reference").is_some());
        }
        if slug == "planning-certificate" {
            assert!(def.field("sections").is_some());
        }
        // Owner fixtures supply the task prerequisites while finance is pending.
        sqlx::query("INSERT INTO submissions(case_id,service_version_id,definition_snapshot_json,definition_sha256,answers_json,submitted_by,submitted_at) VALUES(?,?,?,?,?,?,?)")
            .bind(c.id).bind(c.service_version_id).bind(&snapshot).bind(crate::idempotency::request_hash(snapshot.as_bytes()))
            .bind(json!({"event_name":"Integration event", "setup_notes":"Arrange six tables"}).to_string()).bind(resident.user_id).bind(time::now_str()).execute(&mut *tx).await.unwrap();
        if c.module == "venue_booking" {
            sqlx::query("INSERT INTO bookings(case_id,unit_id,status,start_at,end_at,created_at,updated_at) SELECT ?,id,'requested',?,?,?,? FROM bookable_units WHERE code='rawson-main'")
                .bind(c.id).bind(&start).bind(&end).bind(time::now_str()).bind(time::now_str()).execute(&mut *tx).await.unwrap();
        }
        if c.module == "equipment_hire" {
            sqlx::query("INSERT INTO equipment_requests(case_id,description,requested_hours,site_text,created_at) VALUES(?,'Fictional equipment task',4,'Fictional site',?)")
                .bind(c.id).bind(time::now_str()).execute(&mut *tx).await.unwrap();
        }
        for step in &def.workflow.steps {
            if let Some(handler) = &step.handler {
                handlers.insert(handler.clone());
                if handler.starts_with("operations.") {
                    crate::operations::hooks::step_guard_handler(&mut tx, &c, handler).await.unwrap();
                } else if handler.starts_with("documents.") {
                    documents::hooks::step_guard_handler(&mut tx, &c, handler).await.unwrap();
                } else {
                    assert_eq!(handler, "finance.deposits_settled", "Only finance is deferred");
                }
            }
            for decision in &step.decision_types {
                assert!(
                    ["development_approval", "building_approval", "modification_approval", "planning_certificate"]
                        .contains(&decision.as_str())
                );
                let templates: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM decision_templates WHERE decision_type=? AND active=1")
                        .bind(decision)
                        .fetch_one(&mut *tx)
                        .await
                        .unwrap();
                assert!(templates > 0);
            }
            if let Some(kind) = &step.task_kind {
                tasks.insert(kind.clone());
                let run: i64 = sqlx::query_scalar(
                    "INSERT INTO workflow_step_runs(case_id,step_key,entered_at) VALUES(?,?,?) RETURNING id",
                )
                .bind(c.id)
                .bind(&step.key)
                .bind(time::now_str())
                .fetch_one(&mut *tx)
                .await
                .unwrap();
                let task =
                    crate::operations::api::create_step_task(&mut tx, &c, step, run, resident.db_id()).await.unwrap();
                if kind == "venue_prep" {
                    let instructions: String = sqlx::query_scalar("SELECT instructions FROM tasks WHERE id=?")
                        .bind(task)
                        .fetch_one(&mut *tx)
                        .await
                        .unwrap();
                    assert!(instructions.contains("Integration event") && instructions.contains("Arrange six tables"));
                }
                sqlx::query("UPDATE workflow_step_runs SET left_at=?,left_reason='advanced' WHERE id=?")
                    .bind(time::now_str())
                    .bind(run)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            }
        }
    }
    assert_eq!(tasks.len(), 6);
    assert_eq!(handlers.len(), 7);
    tx.rollback().await.unwrap();
}

async fn authenticated_request(
    state: &AppState,
    credentials: &(String, String),
    method: &str,
    path: &str,
) -> (u16, serde_json::Value) {
    use tower::ServiceExt;
    let req = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("Cookie", format!("nsh_session={}", credentials.0))
        .header("X-CSRF-Token", &credentials.1)
        .body(axum::body::Body::empty())
        .unwrap();
    let response = crate::app::build_router(state.clone()).oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or_default())
}

#[tokio::test]
async fn revoked_org_notifications_recheck_enqueue_delivery_lists_counts_and_reads() {
    let (state, _dir) = fixture().await;
    let mut tx = write_tx(&state.db).await.unwrap();
    let resident = persona(&mut tx, "alexey").await;
    let staff = persona(&mut tx, "olga").await;
    let org: i64 = sqlx::query_scalar(
        "INSERT INTO organisations(name,created_at) VALUES('Notification org','2026-10-07') RETURNING id",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO memberships(organisation_id,user_id,invite_email,role,status,created_at) VALUES(?,?,'org@example.test','member','active','2026-10-07')").bind(org).bind(resident.user_id).execute(&mut *tx).await.unwrap();
    let case = cases::drafts::create_draft(
        &mut tx,
        &resident,
        "rawson-hall-hire",
        cases::drafts::Applicant { applicant_org_id: Some(org), ..Default::default() },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE cases SET status='in_progress' WHERE id=?").bind(case.id).execute(&mut *tx).await.unwrap();
    cases::messages::post_staff_message(&mut tx, &staff, case.id, "Private BEFORE body", None, false).await.unwrap();
    let (in_app,outbound):(i64,i64)=sqlx::query_as("SELECT (SELECT id FROM notifications WHERE case_id=? AND channel='in_app'),(SELECT id FROM notifications WHERE case_id=? AND channel='email')").bind(case.id).bind(case.id).fetch_one(&mut *tx).await.unwrap();
    let credentials = session::create(&mut tx, resident.user_id, true, state.now()).await.unwrap();
    tx.commit().await.unwrap();
    let (_, before) = authenticated_request(&state, &credentials, "GET", "/api/notifications").await;
    assert_eq!(before["unread_count"], 1);
    let mut tx = write_tx(&state.db).await.unwrap();
    sqlx::query("UPDATE memberships SET status='revoked' WHERE organisation_id=? AND user_id=?")
        .bind(org)
        .bind(resident.user_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    cases::messages::post_staff_message(&mut tx, &staff, case.id, "Private AFTER body", None, false).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM notifications WHERE case_id=?")
            .bind(case.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        3
    );
    tx.commit().await.unwrap();
    notify::handle_job(&state, "notify.deliver", &json!({"notification_id":outbound})).await.unwrap();
    let body: String = sqlx::query_scalar("SELECT body FROM notifications WHERE id=?")
        .bind(outbound)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert!(body.is_empty());
    let (status, list) = authenticated_request(&state, &credentials, "GET", "/api/notifications").await;
    assert_eq!(status, 200);
    assert_eq!(list["unread_count"], 0);
    assert!(list["items"].as_array().unwrap().is_empty());
    assert_eq!(
        authenticated_request(&state, &credentials, "POST", &format!("/api/notifications/{in_app}/read")).await.0,
        404
    );
    assert_eq!(authenticated_request(&state, &credentials, "POST", "/api/notifications/read-all").await.0, 204);
    assert!(
        sqlx::query_scalar::<_, Option<String>>("SELECT read_at FROM notifications WHERE id=?")
            .bind(in_app)
            .fetch_one(&state.db)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn inactive_manager_and_owner_do_not_rollback_deadline_sweep_or_applicant_reply() {
    let (state, _dir) = fixture().await;
    let mut tx = write_tx(&state.db).await.unwrap();
    let resident = persona(&mut tx, "alexey").await;
    let manager_id: i64 =
        sqlx::query_scalar("SELECT user_id FROM role_grants WHERE role='manager' AND revoked_at IS NULL LIMIT 1")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let case = cases::drafts::create_draft(&mut tx, &resident, "rawson-hall-hire", Default::default()).await.unwrap();
    sqlx::query("UPDATE cases SET status='in_progress' WHERE id=?").bind(case.id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO case_assignments(case_id,user_id,role,assigned_at) VALUES(?,?,'owner','2026-10-07')")
        .bind(case.id)
        .bind(manager_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO deadlines(case_id,kind,label,basis,duration_days,pausable,started_at,due_at,status,policy_json) VALUES(?,'response','Response','calendar',1,0,'2000-01-01T00:00:00Z','2000-01-02T00:00:00Z','running','{}')").bind(case.id).execute(&mut *tx).await.unwrap();
    // Legacy deactivation left active grants/assignments behind.
    sqlx::query("UPDATE users SET is_active=0 WHERE id=?").bind(manager_id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    crate::deadlines::handle_job(&state, "deadline.sweep", &json!({})).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM deadlines WHERE case_id=?")
            .bind(case.id)
            .fetch_one(&state.db)
            .await
            .unwrap(),
        "breached"
    );
    let mut tx = write_tx(&state.db).await.unwrap();
    cases::messages::applicant_reply(&mut tx, &state, &resident, case.id, "Applicant reply", None).await.unwrap();
    users::deactivate_user(&mut tx, manager_id).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM case_assignments WHERE user_id=? AND ended_at IS NULL")
            .bind(manager_id)
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
        0
    );
}
