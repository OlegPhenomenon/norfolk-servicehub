//! Shared operations endpoints answer each audience with its own actions and projection.
mod support;
use chrono::Duration;
use serde_json::json;
use servicehub::seed::scenarios;

#[tokio::test]
async fn only_the_assigned_field_worker_records_a_task_result() {
    let (mut d, _dir) = support::fixture().await;
    let (_, answers) = d.answers("olga", "road-issue").await.unwrap();
    let c = d.req("olga","POST","/api/staff/intake",json!({"service":"road-issue","channel":"phone","applicant_name":"Fictional caller","applicant_email":"caller@example.invalid","applicant_phone":"+672355512","answers":answers})).await.unwrap()["id"].as_i64().unwrap();
    d.action("olga", c, "advance").await.unwrap();
    let tasks = d.req("olga", "GET", &format!("/api/cases/{c}/tasks"), json!({})).await.unwrap();
    let task = tasks.as_array().unwrap().iter().find(|t| t["kind"] == "road_inspection").unwrap().clone();
    assert_eq!(task["assigned_to"], d.people["jake"].user_id);
    assert_eq!(task["can_manage"], true);
    let id = task["id"].as_i64().unwrap();
    for who in ["olga", "helen"] {
        // Managing staff keep a read-only view of the task but cannot record the field work.
        let t = d.req(who, "GET", &format!("/api/field/tasks/{id}"), json!({})).await.unwrap();
        for (kind, body) in [("result", "Recorded from the office"), ("status", "done"), ("note", "Office note")] {
            let command = d.key();
            d.expect(
                who,
                "POST",
                &format!("/api/field/tasks/{id}/updates"),
                json!({"client_command_id":command,"expected_revision":t["revision"],"kind":kind,"body":body}),
                403,
            )
            .await
            .unwrap();
        }
    }
    let untouched = d.req("helen", "GET", &format!("/api/field/tasks/{id}"), json!({})).await.unwrap();
    assert_eq!(untouched["revision"], task["revision"]);
    assert!(untouched["result_text"].is_null());
    d.complete_task(c, "road_inspection").await.unwrap();
    assert_eq!(d.req("helen", "GET", &format!("/api/field/tasks/{id}"), json!({})).await.unwrap()["status"], "done");
}

#[tokio::test]
async fn applicant_equipment_detail_has_no_staff_identifiers() {
    let (mut d, _dir) = support::fixture().await;
    let (c,_)=d.submit("ben","equipment-hire",json!({"request":{"description":"Bobcat","requested_hours":4,"preferred_date":"2026-10-09","site_text":"Fictional depot"}}),None).await.unwrap();
    d.action("olga", c, "advance").await.unwrap();
    let start = d.instant(2, 7, 30);
    let end = d.instant(2, 13, 0);
    scenarios::schedule_equipment(&mut d, c, &start, &end).await.unwrap();
    d.clock.advance(Duration::days(3));
    for who in ["ben", "olga", "jake", "tom"] {
        d.login(who).await.unwrap();
    }
    scenarios::finish_equipment(&mut d, c, &start, &end).await.unwrap();
    let ben = d.req("ben", "GET", &format!("/api/cases/{c}/equipment"), json!({})).await.unwrap();
    for key in ["task_id", "operator_user_id", "assigned_resource_id"] {
        assert!(ben["request"].get(key).is_none(), "request.{key}");
    }
    let usage = &ben["usage"][0];
    for key in ["operator_user_id", "recorded_by", "approved_by", "client_command_id", "expenses_note", "resource_id"] {
        assert!(usage.get(key).is_none(), "usage.{key}");
    }
    assert_eq!(usage["billable_minutes"], 300);
    assert!(usage["approved_at"].is_string());
    assert_eq!(ben["can_schedule"], false);
    let staff = d.req("tom", "GET", &format!("/api/cases/{c}/equipment"), json!({})).await.unwrap();
    assert!(staff["request"]["task_id"].is_i64());
    assert!(staff["usage"][0]["operator_user_id"].is_i64());
    // Finance reads the case's tasks without assign/cancel rights.
    let tasks = d.req("tom", "GET", &format!("/api/cases/{c}/tasks"), json!({})).await.unwrap();
    assert!(tasks.as_array().unwrap().iter().all(|t| t["can_manage"] == false));
}

#[tokio::test]
async fn confidential_complaint_notices_address_the_staff_handler_as_staff() {
    let (mut d, _dir) = support::fixture().await;
    let (c, _) = d.submit("alexey", "complaint", json!({}), None).await.unwrap();
    let number = d.detail("ruth", c).await.unwrap()["case"]["number"].as_str().unwrap().to_string();
    let ruth: Vec<(String, String)> = sqlx::query_as(
        "SELECT n.subject,n.body FROM notifications n JOIN users u ON u.id=n.user_id WHERE u.persona_key='ruth' AND n.case_id=?",
    )
    .bind(c)
    .fetch_all(&d.state.db)
    .await
    .unwrap();
    assert!(ruth.iter().any(|(subject, _)| *subject == format!("Confidential complaint {number} assigned to you")));
    assert!(ruth.iter().all(|(s, b)| !s.contains("your feedback") && !b.contains("your feedback")), "{ruth:?}");
}
