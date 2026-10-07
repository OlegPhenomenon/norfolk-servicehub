use super::{api, bookings, calendar, equipment, hooks, model, tasks};
use crate::{
    auth::{Actor, RoleGrant, UserKind},
    authz::Role,
    cases::core::{self, NewCase},
    db,
    error::ErrorCode,
    state::test_support::test_state,
    time,
};
use serde_json::json;
use sqlx::SqliteConnection;

async fn fixture() -> (crate::state::AppState, tempfile::TempDir) {
    let (st, dir) = test_state().await;
    let mut tx = db::write_tx(&st.db).await.unwrap();
    super::seed(&mut tx, &st).await.unwrap();
    super::seed(&mut tx, &st).await.unwrap();
    sqlx::query("INSERT INTO users(id,email,display_name,kind,persona_key,created_at) VALUES (1,'private@example.invalid','Private Applicant','resident','alexey','2026-10-07'),(2,'worker@example.invalid','Jake Fictional','staff','jake','2026-10-07')").execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO role_grants(user_id,role,granted_at) VALUES (2,'field_worker','2026-10-07')")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO services(id,slug,name,category,module,department,created_at) VALUES (1,'rawson-hall-hire','Hall','Venues','venue_booking','Customer Care','2026-10-07')").execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO service_versions(id,service_id,version,status,definition_json,created_at) VALUES (1,1,1,'published','{}','2026-10-07')").execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    (st, dir)
}
async fn make_case(tx: &mut SqliteConnection, unit: &str) -> i64 {
    let c = core::create_case(
        tx,
        NewCase {
            service_id: 1,
            service_version_id: 1,
            module: "venue_booking".into(),
            title: "Fictional family gathering".into(),
            status: "submitted".into(),
            applicant_user_id: Some(1),
            applicant_org_id: None,
            applicant_name: "Private Applicant".into(),
            applicant_email: Some("private@example.invalid".into()),
            applicant_phone: Some("secret-phone".into()),
            intake_channel: "online".into(),
            recorded_by_user_id: None,
            property_ref: None,
        },
    )
    .await
    .unwrap();
    core::assign_number(tx, c.id).await.unwrap();
    let u = model::unit(tx, unit).await.unwrap();
    sqlx::query("INSERT INTO bookings(case_id,unit_id,status,start_at,end_at,attendees,created_at,updated_at) VALUES (?,?,'requested','2030-11-14T06:00:00.000Z','2030-11-14T11:00:00.000Z',80,'2026-10-07','2026-10-07')").bind(c.id).bind(u.id).execute(&mut *tx).await.unwrap();
    let b = model::booking(tx, c.id).await.unwrap();
    model::revision(tx, &b, None, "Requested").await.unwrap();
    c.id
}
fn worker() -> Actor {
    Actor {
        user_id: 2,
        kind: UserKind::Staff,
        roles: vec![RoleGrant { role: Role::FieldWorker, scope_service_id: None }],
        display_name: "Jake".into(),
        mfa_passed: true,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_whole_and_main_confirmation_exactly_one_wins() {
    let (st, _dir) = fixture().await;
    let mut tx = db::write_tx(&st.db).await.unwrap();
    let whole = make_case(&mut tx, "rawson-whole").await;
    let main = make_case(&mut tx, "rawson-main").await;
    tx.commit().await.unwrap();
    let run = |case| {
        let db = st.db.clone();
        tokio::spawn(async move {
            let mut tx = db::write_tx(&db).await.unwrap();
            let result = bookings::confirm_allocation(&mut tx, case, 1, None, "Fictional booking").await;
            if result.is_ok() {
                tx.commit().await.unwrap();
            }
            result
        })
    };
    let a = run(whole);
    let b = run(main);
    let a = a.await.unwrap();
    let b = b.await.unwrap();
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let err = a.err().or(b.err()).unwrap();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(err.message.contains("NSH-"));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bookings WHERE status='confirmed'").fetch_one(&st.db).await.unwrap();
    assert_eq!(count, 1);
    let revisions: i64 = sqlx::query_scalar("SELECT count(*) FROM booking_revisions").fetch_one(&st.db).await.unwrap();
    assert_eq!(revisions, 3);
}
#[tokio::test]
async fn failed_reschedule_keeps_original_occupancy_and_history() {
    let (st, _dir) = fixture().await;
    let mut tx = db::write_tx(&st.db).await.unwrap();
    let id = make_case(&mut tx, "rawson-main").await;
    let b = bookings::confirm_allocation(&mut tx, id, 1, None, "Original").await.unwrap();
    let u = model::unit(&mut tx, "rawson-main").await.unwrap();
    let r = model::unit_resources(&mut tx, u.id).await.unwrap()[0].id;
    sqlx::query("INSERT INTO occupancies(resource_id,source,label,start_at,end_at,created_at) VALUES (?,'maintenance','Target blocked','2030-11-15T05:00:00.000Z','2030-11-15T12:00:00.000Z','2026-10-07')").bind(r).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = db::write_tx(&st.db).await.unwrap();
    let mut changed = b.clone();
    let s = model::Slot {
        unit_code: u.code.clone(),
        start_at: "2030-11-15T06:00:00Z".into(),
        end_at: "2030-11-15T11:00:00Z".into(),
        attendees: 80,
    };
    assert_eq!(
        bookings::move_allocation(&mut tx, &mut changed, &u, &s, None, "Move", "New").await.unwrap_err().code,
        ErrorCode::Conflict
    );
    tx.rollback().await.unwrap();
    let mut conn = st.db.acquire().await.unwrap();
    let current = model::booking(&mut conn, id).await.unwrap();
    assert_eq!(current.start_at, b.start_at);
    assert_eq!(current.revision, 2);
    let active: i64 = sqlx::query_scalar("SELECT count(*) FROM occupancies WHERE booking_id=? AND active=1")
        .bind(b.id)
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(active, 1);
    // Touching a buffered interval is free; one millisecond inside it conflicts.
    let free = model::conflicts(&mut conn, u.id, "2030-11-14T13:00:00Z", "2030-11-14T14:00:00Z", None).await.unwrap();
    assert!(free.is_empty());
    assert!(
        !model::conflicts(&mut conn, u.id, "2030-11-14T12:59:59Z", "2030-11-14T14:00:00Z", None)
            .await
            .unwrap()
            .is_empty()
    );
}
#[test]
fn norfolk_hours_midnight_and_exact_usage() {
    let s = model::Slot {
        unit_code: "rawson-main".into(),
        start_at: "2030-11-14T06:00:00Z".into(),
        end_at: "2030-11-14T12:00:00Z".into(),
        attendees: 80,
    };
    model::slot_times(&s, time::parse("2026-10-07T00:00:00Z").unwrap()).unwrap();
    assert_eq!(model::day_count(&s.start_at, &s.end_at).unwrap(), 1);
    let mut bad = s.clone();
    bad.end_at = "2030-11-14T12:01:00Z".into();
    assert!(model::slot_times(&bad, time::parse("2026-10-07T00:00:00Z").unwrap()).is_err());
    bad = s.clone();
    bad.start_at = "2030-11-13T18:00:00Z".into();
    assert!(model::slot_times(&bad, time::parse("2026-10-07T00:00:00Z").unwrap()).is_err());
    assert_eq!(equipment::billable("2026-10-07T07:30:00Z", "2026-10-07T13:30:00Z", 30).unwrap(), 330);
    assert!(equipment::billable("2026-10-07T07:30:00Z", "2026-10-07T13:30:00Z", 361).is_err());
    assert!(equipment::billable("2026-10-07T07:30:00Z", "2026-10-07T13:30:01Z", 30).is_err());
    assert!(hooks::validate_location(&json!({"lat":-29.04,"lng":167.95,"description":"Near school"})).is_ok());
    assert!(hooks::validate_location(&json!({"lat":-30.0,"lng":167.95,"description":"Off island"})).is_err());
}
#[tokio::test]
async fn task_projection_idempotency_stale_revision_and_run_isolation() {
    let (st, _dir) = fixture().await;
    let mut tx = db::write_tx(&st.db).await.unwrap();
    let id = make_case(&mut tx, "rawson-main").await;
    let answers = json!({"event_name":"Fictional family gathering","setup_notes":"Arrange 8 tables","email":"private@example.invalid","phone":"secret-phone","documents":["private-passport"]});
    sqlx::query("INSERT INTO submissions(case_id,service_version_id,definition_sha256,submitted_at,submitted_by,answers_json,definition_snapshot_json) VALUES (?,1,'demo','2026-10-07',1,?,'{}')").bind(id).bind(answers.to_string()).execute(&mut *tx).await.unwrap();
    let run: i64 = sqlx::query_scalar(
        "INSERT INTO workflow_step_runs(case_id,step_key,entered_at) VALUES (?,'prep','2026-10-07') RETURNING id",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let step =
        serde_json::from_value(json!({"key":"prep","kind":"task","label":"Prepare hall","task_kind":"venue_prep"}))
            .unwrap();
    let created_case = core::load_case(&mut tx, id).await.unwrap();
    let task = api::create_step_task(&mut tx, &created_case, &step, run, None).await.unwrap();
    let c = core::load_case(&mut tx, id).await.unwrap();
    assert_eq!(api::create_step_task(&mut tx, &c, &step, run, None).await.unwrap(), task);
    let t = tasks::require(&mut tx, &worker(), task, true).await.unwrap();
    let projection = tasks::projection(&mut tx, &t).await.unwrap();
    let raw = projection.to_string();
    for private in ["private@example.invalid", "secret-phone", "private-passport", "applicant_name"] {
        assert!(!raw.contains(private), "{raw}");
    }
    assert!(raw.contains("Arrange 8 tables"));
    assert!(!api::step_tasks_done(&mut tx, id, run).await.unwrap());
    assert!(!api::step_tasks_done(&mut tx, id, run + 1).await.unwrap());
    let v = json!({"client_command_id":"offline-1","kind":"result","body":"Prepared","expected_revision":1,"created_offline_at":"2026-10-07T00:00:00Z"});
    let response = tasks::apply_update(&mut tx, &worker(), &t, &v, None).await.unwrap();
    let scope = format!("operations.task.{task}");
    tasks::store_command(&mut tx, &worker(), &v, &scope, &response).await.unwrap();
    assert_eq!(
        tasks::replay(&mut tx, &worker(), task, &v, &scope).await.unwrap().unwrap()["status"],
        "already_applied"
    );
    let mut changed = v.clone();
    changed["body"] = json!("Other");
    assert_eq!(
        tasks::replay(&mut tx, &worker(), task, &changed, &scope).await.unwrap_err().code,
        ErrorCode::IdempotencyMismatch
    );
    let current = tasks::load(&mut tx, task).await.unwrap();
    let e = tasks::apply_update(
        &mut tx,
        &worker(),
        &current,
        &json!({"client_command_id":"offline-2","kind":"status","body":"done","expected_revision":1}),
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::StaleRevision);
    assert!(e.fields["current_state"].contains("revision"));
    sqlx::query("UPDATE tasks SET status='done' WHERE id=?").bind(task).execute(&mut *tx).await.unwrap();
    assert!(api::step_tasks_done(&mut tx, id, run).await.unwrap());
    sqlx::query(
        "INSERT INTO case_access_denials(case_id,user_id,reason,created_at) VALUES (?,2,'Denied','2026-10-07')",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .unwrap();
    assert_eq!(tasks::require(&mut tx, &worker(), task, false).await.unwrap_err().code, ErrorCode::NotFound);
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn public_road_projection_never_contains_reporter_text_or_names() {
    let (st, _dir) = fixture().await;
    let mut tx = db::write_tx(&st.db).await.unwrap();
    let id = make_case(&mut tx, "rawson-main").await;
    sqlx::query("UPDATE cases SET module='road_issue',public_map=1,location_lat=-29.04,location_lng=167.95,location_text='SECRET REPORTER TEXT' WHERE id=?").bind(id).execute(&mut *tx).await.unwrap();
    let out = calendar::public_roads(&mut tx).await.unwrap();
    let row = &out[0];
    assert_eq!(
        row.as_object().unwrap().keys().cloned().collect::<Vec<_>>(),
        vec!["category", "id", "location", "reported_on", "status_text"]
    );
    let text = out.to_string();
    for private in ["SECRET REPORTER TEXT", "Private Applicant", "private@example.invalid", "secret-phone"] {
        assert!(!text.contains(private));
    }
    assert!(row["location"]["description"].is_null());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM resources").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(count, 6);
}
