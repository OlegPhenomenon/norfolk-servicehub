//! Seed catalogue invariants, complete logical reset repeatability and visitor availability.
use chrono::Duration;
use serde_json::json;
use servicehub::{
    AppState,
    clock::{self, FixedClock},
    config::Config,
    db,
    seed::{self, driver::Driver, scenarios},
    time,
};
use std::{collections::BTreeMap, sync::Arc};

async fn counts(state: &AppState) -> BTreeMap<String, i64> {
    let tables:Vec<String>=sqlx::query_scalar("SELECT name FROM pragma_table_list WHERE schema='main' AND type IN ('table','virtual') AND name NOT LIKE 'sqlite_%' ORDER BY name").fetch_all(&state.db).await.unwrap();
    let mut counts = BTreeMap::new();
    for table in tables {
        let n = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM \"{}\"", table.replace('"', "\"\"")))
            .fetch_one(&state.db)
            .await
            .unwrap();
        counts.insert(table, n);
    }
    counts
}
async fn scalar(state: &AppState, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&state.db).await.unwrap()
}

#[tokio::test]
async fn seeded_history_is_complete_balanced_repeatable_and_leaves_the_visitor_story_free() {
    let dir = tempfile::tempdir().unwrap();
    let now = time::parse("2026-10-07T03:15:00Z").unwrap();
    let state = AppState::with_clock(Config::for_tests(dir.path()), Arc::new(FixedClock::new(now))).await.unwrap();
    db::migrate(&state.db).await.unwrap();
    let started = std::time::Instant::now();
    clock::scope(state.clock.clone(), seed::reset_demo(&state)).await.unwrap();
    assert!(started.elapsed().as_secs_f64() < 15.0, "seed took {:?}", started.elapsed());
    let first = counts(&state).await;
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM notifications n JOIN users u ON u.id=n.user_id WHERE u.kind='staff' AND n.channel='in_app' AND n.created_at<'2026-10-07T03:15:00Z' AND n.read_at IS NULL").await, 0);
    assert!(scalar(&state, "SELECT COALESCE(MAX(unread),0) FROM (SELECT COUNT(*) AS unread FROM notifications n JOIN users u ON u.id=n.user_id WHERE u.kind='staff' AND n.channel='in_app' AND n.read_at IS NULL GROUP BY n.user_id)").await <= 3);
    assert!(scalar(&state, "SELECT COUNT(*) FROM notifications n JOIN users u ON u.id=n.user_id WHERE u.persona_key='jake' AND n.channel='in_app' AND n.read_at IS NOT NULL").await > 0);
    assert_eq!(
        scalar(&state, "SELECT COUNT(*) FROM bookings b JOIN cases c ON c.id=b.case_id WHERE c.status='completed'")
            .await,
        3
    );
    assert_eq!(
        scalar(&state, "SELECT COUNT(*) FROM deposit_decisions WHERE retain_cents=5000 AND refund_cents=20000").await,
        1
    );
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM refunds WHERE status='completed'").await, 3);
    assert_eq!(scalar(&state,"SELECT COUNT(*) FROM bookings b JOIN cases c ON c.id=b.case_id WHERE b.status='confirmed' AND c.status<>'completed'").await,3);
    assert_eq!(scalar(&state,"SELECT COUNT(*) FROM bookings b JOIN cases c ON c.id=b.case_id WHERE b.status='requested' AND c.current_step='payment'").await,1);
    assert_eq!(scalar(&state,"SELECT COUNT(*) FROM decisions WHERE decision_type IN ('development_approval','building_approval') AND status='issued'").await,2);
    assert_eq!(scalar(&state,"SELECT COUNT(*) FROM decision_evidence e JOIN document_versions v ON v.id=e.document_version_id JOIN decisions d ON d.id=e.decision_id WHERE d.decision_type IN ('development_approval','building_approval') AND v.version=2").await,2);
    assert_eq!(scalar(&state,"SELECT COUNT(*) FROM cases c JOIN services s ON s.id=c.service_id WHERE s.slug='building-commencement-notice' AND c.building_project_id IS NOT NULL AND c.status='completed'").await,1);
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM case_links WHERE kind='modification_of'").await, 1);
    // Audit 2 / N-05: 18 catalogue services (17 published), all seed-managed; a Stage B notice on the seeded
    // project was returned, replaced and accepted against version 2; a pipeline crossing was decided.
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM services").await, 18);
    assert_eq!(
        scalar(&state, "SELECT COUNT(*) FROM service_versions WHERE status='published' AND seed_hash IS NOT NULL")
            .await,
        17
    );
    assert_eq!(scalar(&state,"SELECT COUNT(*) FROM cases c JOIN services s ON s.id=c.service_id WHERE s.slug='builder-stage-b-notice' AND c.status='completed' AND c.building_project_id=(SELECT building_project_id FROM cases c2 JOIN services s2 ON s2.id=c2.service_id WHERE s2.slug='building-commencement-notice')").await,1);
    assert_eq!(scalar(&state,"SELECT COUNT(*) FROM document_comments k JOIN document_versions v ON v.id=k.document_version_id JOIN documents d ON d.id=v.document_id JOIN cases c ON c.id=d.case_id JOIN services s ON s.id=c.service_id WHERE s.slug='builder-stage-b-notice' AND k.request_new_version=1 AND k.resolved_by_version_id IS NOT NULL").await,1);
    assert_eq!(scalar(&state,"SELECT COUNT(*) FROM decision_evidence e JOIN document_versions v ON v.id=e.document_version_id JOIN decisions d ON d.id=e.decision_id JOIN cases c ON c.id=d.case_id JOIN services s ON s.id=c.service_id WHERE s.slug='builder-stage-b-notice' AND d.decision_type='service_response' AND d.status='issued' AND v.version=2").await,1);
    assert_eq!(scalar(&state,"SELECT COUNT(*) FROM decisions d JOIN cases c ON c.id=d.case_id JOIN services s ON s.id=c.service_id WHERE s.slug='pipeline-conduit-crossing' AND d.status='issued' AND c.status='completed'").await,1);
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM exhibitions WHERE status='open'").await, 1);
    assert_eq!(
        scalar(
            &state,
            "SELECT COUNT(*) FROM exhibition_items WHERE published_blob_id IS NOT NULL AND redactions_json<>'[]'"
        )
        .await,
        2
    );
    // Seeded building route (audit N-01..N-03): fee assessed and paid, scope confirmed, exhibition closed and
    // its comment considered before the decisions; the modification is assessed, paid and on exhibition.
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM building_fee_assessments").await, 2);
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM cases c JOIN services s ON s.id=c.service_id WHERE s.slug IN ('development-application','modify-approval') AND NOT EXISTS(SELECT 1 FROM finance_line_balances l JOIN invoices i ON i.id=l.invoice_id WHERE i.case_id=c.id AND l.amount_cents-l.credited_cents-l.paid_cents>0) AND EXISTS(SELECT 1 FROM invoices i WHERE i.case_id=c.id AND i.kind='invoice')").await, 2);
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM building_approval_scopes WHERE source='staff'").await, 1);
    assert_eq!(
        scalar(&state, "SELECT COUNT(*) FROM public_submissions WHERE status='considered' AND outcome IS NOT NULL")
            .await,
        1
    );
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM cases c JOIN services s ON s.id=c.service_id WHERE s.slug='development-application' AND c.status='completed' AND EXISTS(SELECT 1 FROM building_approval_scopes b WHERE b.case_id=c.id AND b.source='staff') AND EXISTS(SELECT 1 FROM building_fee_assessments f WHERE f.case_id=c.id)").await, 1);
    assert_eq!(
        scalar(&state, "SELECT COUNT(*) FROM decisions WHERE decision_type='planning_certificate' AND status='issued'")
            .await,
        1
    );
    assert_eq!(
        scalar(&state, "SELECT COUNT(*) FROM cases WHERE module='planning_certificate' AND current_step='payment'")
            .await,
        1
    );
    assert_eq!(
        scalar(
            &state,
            "SELECT COUNT(*) FROM equipment_usage WHERE billable_minutes=300 AND final_invoice_id IS NOT NULL"
        )
        .await,
        1
    );
    assert_eq!(scalar(&state,"SELECT COUNT(*) FROM equipment_requests WHERE status='scheduled' AND id NOT IN (SELECT equipment_request_id FROM equipment_usage)").await,1);
    assert_eq!(scalar(&state,"SELECT COUNT(*) FROM cases c WHERE c.module='road_issue' AND c.applicant_user_id=(SELECT id FROM users WHERE persona_key='ben')").await,6);
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM case_links WHERE kind='duplicate_of'").await, 1);
    assert_eq!(
        scalar(
            &state,
            "SELECT COUNT(*) FROM case_access_denials WHERE user_id=(SELECT id FROM users WHERE persona_key='olga')"
        )
        .await,
        2
    );
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM case_links WHERE kind='review_of'").await, 1);
    assert_eq!(scalar(&state,"SELECT COUNT(*) FROM cases WHERE recorded_by_user_id=(SELECT id FROM users WHERE persona_key='olga') AND applicant_user_id IS NULL AND intake_channel IN ('phone','walk_in')").await,4);
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM cases WHERE reopened_count>0").await, 1);
    assert!(scalar(&state, "SELECT COUNT(*) FROM deadlines WHERE status='breached'").await > 0);
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM legacy_import_batches WHERE status='imported'").await, 1);
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM legacy_import_records WHERE case_id IS NOT NULL").await, 12);
    assert!(scalar(&state, "SELECT COUNT(*) FROM audit_log WHERE action='records.integration_retry'").await > 0);
    assert_eq!(scalar(&state,"SELECT COUNT(*) FROM (SELECT system_code,operation_id FROM mock_external_records GROUP BY system_code,operation_id HAVING COUNT(*)>1)").await,0);
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM sessions").await, 0);
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM integration_deliveries WHERE status<>'accepted'").await, 0);
    assert_eq!(scalar(&state,"SELECT COUNT(*) FROM cases WHERE module='venue_booking' AND applicant_user_id=(SELECT id FROM users WHERE persona_key='alexey') AND status IN ('draft','submitted','in_progress','waiting_on_applicant')").await,1);
    let negative_durations: i64 = scalar(
        &state,
        "SELECT COUNT(*) FROM cases WHERE closed_at IS NOT NULL AND julianday(closed_at)<julianday(submitted_at)",
    )
    .await;
    assert_eq!(negative_durations, 0);
    assert_eq!(
        scalar(&state, "SELECT COUNT(*) FROM case_events WHERE julianday(at)>julianday('2026-10-07T03:20:00Z')").await,
        0
    );
    assert_eq!(scalar(&state, "SELECT COUNT(*) FROM pragma_foreign_key_check").await, 0);

    // Calling the history runner again is a no-op; actual CLI seed-demo resets through this path.
    scenarios::run(&state).await.unwrap();
    assert_eq!(counts(&state).await, first);
    clock::scope(state.clock.clone(), seed::reset_demo(&state)).await.unwrap();
    assert_eq!(counts(&state).await, first);
    let mut d = Driver::new(state.clone(), now + Duration::minutes(1)).await.unwrap();
    for who in ["helen", "tom", "olga", "alexey"] {
        d.login(who).await.unwrap();
    }
    scenarios::verify(&mut d).await.unwrap();
    let search = d.req("alexey", "GET", "/api/public/services?q=birthday%20party", json!({})).await.unwrap();
    assert!(search["items"].as_array().unwrap().iter().any(|s| s["module"] == "venue_booking"));
    let dashboard = d.req("helen", "GET", "/api/staff/dashboard", json!({})).await.unwrap();
    assert!(dashboard["metrics"]["completed"].as_i64().unwrap() > 0);
    assert!(dashboard["metrics"]["overdue"].as_i64().unwrap() > 0);
    for service in dashboard["services"].as_array().unwrap() {
        for metric in servicehub::records::metrics::METRICS {
            let drill = d
                .req(
                    "helen",
                    "GET",
                    &format!("/api/staff/dashboard/metrics/{metric}?service_id={}", service["service_id"]),
                    json!({}),
                )
                .await
                .unwrap();
            assert_eq!(service["metrics"][metric].as_u64().unwrap(), drill["items"].as_array().unwrap().len() as u64);
        }
    }
    for person in dashboard["workload"].as_array().unwrap() {
        for metric in ["open", "overdue"] {
            let drill = d
                .req(
                    "helen",
                    "GET",
                    &format!("/api/staff/dashboard/metrics/{metric}?owner_id={}", person["user_id"]),
                    json!({}),
                )
                .await
                .unwrap();
            assert_eq!(person[metric].as_u64().unwrap(), drill["items"].as_array().unwrap().len() as u64);
        }
    }
    let roads = d.req("alexey", "GET", "/api/public/road-issues", json!({})).await.unwrap();
    assert!(roads.as_array().unwrap().len() >= 6);
    // Weeks 2–6 have ample whole-venue dates; a base catalogue maintenance window may block one day.
    let mut free_whole_days = 0;
    for day in 14..=42 {
        let from = d.instant(day, 10, 0);
        let to = d.instant(day, 16, 0);
        let availability = d
            .req(
                "alexey",
                "GET",
                &format!("/api/public/venues/rawson-hall/availability?from={from}&to={to}"),
                json!({}),
            )
            .await
            .unwrap();
        assert!(
            availability["units"]
                .as_array()
                .unwrap()
                .iter()
                .any(|u| u["active"] == true && u["busy"].as_array().unwrap().is_empty()),
            "Day {day}: {availability}"
        );
        if availability["units"]
            .as_array()
            .unwrap()
            .iter()
            .all(|u| u["active"] == true && u["busy"].as_array().unwrap().is_empty())
        {
            free_whole_days += 1;
        }
    }
    assert!(free_whole_days >= 27, "Only {free_whole_days} free whole-hall dates");
    let hall = d.hall("rawson-whole", 21);
    d.submit("alexey", "rawson-hall-hire", hall, None).await.unwrap();
    // The shipped statement is immediately usable against the seeded unpaid hall/certificate.
    let csv = include_str!("../seed-data/statements/demo-statement.csv");
    let statement = d
        .req("tom", "POST", "/api/finance/statements", json!({"filename":"demo-statement.csv","csv":csv}))
        .await
        .unwrap();
    let statuses: Vec<_> =
        statement["rows"].as_array().unwrap().iter().map(|r| r["status"].as_str().unwrap()).collect();
    assert_eq!(statuses, vec!["matched", "unmatched", "unmatched", "duplicate"]);
    // A visitor can settle Alexey's seeded past bond at the real demo time, without advancing to the event.
    let queue = d.req("tom", "GET", "/api/finance/deposits", json!({})).await.unwrap();
    let row = queue.as_array().unwrap().iter().find(|r| r["applicant_name"] == "Alexey Turner").unwrap();
    let case = row["case_id"].as_i64().unwrap();
    assert_eq!(d.money("tom", case).await.unwrap()["deposit_ready"], true);
    d.req("tom","POST", &format!("/api/cases/{case}/deposit-decision"),json!({"invoice_line_id":row["invoice_line_id"],"refund_cents":20000,"retain_items":[{"label":"Extra cleaning","cents":5000}],"reason":"Live visitor partial retention","expected_revision":row["case_revision"]})).await.unwrap();
    assert_eq!(d.money("tom", case).await.unwrap()["refunds"][0]["status"], "processing");
    d.drain().await.unwrap();
    d.clock.advance(Duration::seconds(4));
    d.drain().await.unwrap();
    assert_eq!(d.money("alexey", case).await.unwrap()["refunds"][0]["status"], "completed");
    assert_eq!(d.detail("alexey", case).await.unwrap()["case"]["status"], "completed");
}
