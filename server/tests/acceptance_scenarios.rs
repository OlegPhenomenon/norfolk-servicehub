//! Brief §7: complete HTTP journeys with isolated databases, real sessions/TOTP and deterministic jobs.
mod support;
use axum::{body::Body, http::Request};
use chrono::Duration;
use serde_json::{Value, json};
use servicehub::seed::{driver::Driver, scenarios};
use tower::ServiceExt;

async fn pdf(d: &mut Driver, who: &str, path: &str) -> Vec<u8> {
    let (status, bytes) = d.raw(who, "GET", path, "text/plain", vec![], &[]).await.unwrap();
    assert_eq!(status, 200, "{path}: {}", String::from_utf8_lossy(&bytes));
    assert!(bytes.starts_with(b"%PDF-"));
    bytes
}
async fn confirm(d: &mut Driver, c: i64, who: &str) {
    d.action("olga", c, "advance").await.unwrap();
    d.pay(who, c, false).await.unwrap();
    let b = d.req("olga", "GET", &format!("/api/cases/{c}/booking"), json!({})).await.unwrap();
    d.req(
        "olga",
        "POST",
        &format!("/api/cases/{c}/booking/confirm"),
        json!({"expected_revision":b["booking"]["revision"]}),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn new_service_new_resident_and_immutable_v1_after_v2_publication() {
    let (mut d, _dir) = support::fixture().await;
    let service=d.req("mark","POST","/api/admin/services",json!({"slug":"community-tree-advice","name":"Community tree advice","category":"Environment","department":"Customer Care","module":"generic"})).await.unwrap();
    let id = service["id"].as_i64().unwrap();
    let v1 = service["version_id"].as_i64().unwrap();
    let mut def = json!({"module":"generic","summary":"Request fictional tree advice.","outcome":"A recorded assessment and response.","fields":[{"key":"tree","type":"text","label":"Tree location v1","required":true},{"key":"consent","type":"checkbox","label":"I confirm these details","required":true}],"documents":[{"key":"plan","label":"Tree plan","required":true,"accept":["application/pdf"]}],"workflow":{"steps":[{"key":"intake","kind":"review","role":"intake","label":"Olga assesses","applicant_label":"Being assessed"},{"key":"done","kind":"complete","label":"Complete","applicant_label":"Advice complete"}]},"deadlines":[],"pricing":[]});
    d.req("mark", "PUT", &format!("/api/admin/services/{id}/versions/{v1}"), def.clone()).await.unwrap();
    d.req("mark", "POST", &format!("/api/admin/services/{id}/versions/{v1}/publish"), json!({})).await.unwrap();
    d.req("new-resident", "GET", "/api/me", json!({})).await.unwrap();
    d.expect("new-resident","POST","/api/auth/register",json!({"name":"Fictional new resident","email":"new-resident@example.invalid","password":"Long-enough-fictional-password-47!"}),201).await.unwrap();
    let (c, docs) = d
        .submit(
            "new-resident",
            "community-tree-advice",
            json!({"tree":"Fictional oak near the library","consent":true}),
            None,
        )
        .await
        .unwrap();
    let frozen = d.detail("new-resident", c).await.unwrap();
    let version = docs["plan"]["version_id"].as_i64().unwrap();
    let original_pdf = pdf(&mut d, "new-resident", &format!("/api/document-versions/{version}/download")).await;
    let revision = d.revision("olga", c).await.unwrap();
    let olga = d.people["olga"].user_id;
    d.req("olga","POST",&format!("/api/cases/{c}/assign"),json!({"user_id":olga,"role":"owner","reason":"Assess the tree advice request","replace_owner":true,"expected_revision":revision})).await.unwrap();
    let revision = d.revision("olga", c).await.unwrap();
    d.req(
        "olga",
        "POST",
        &format!("/api/cases/{c}/messages"),
        json!({"body":"Advice: arrange a qualified tree assessment before any works.","expected_revision":revision}),
    )
    .await
    .unwrap();
    d.action("olga", c, "advance").await.unwrap();
    assert_eq!(d.detail("new-resident", c).await.unwrap()["case"]["status"], "completed");
    let v2 = d.req("mark", "POST", &format!("/api/admin/services/{id}/versions"), json!({})).await.unwrap()["id"]
        .as_i64()
        .unwrap();
    def["fields"][0]["label"] = json!("Tree location v2");
    def["fields"]
        .as_array_mut()
        .unwrap()
        .push(json!({"key":"species","type":"text","label":"Species","required":true}));
    d.req("mark", "PUT", &format!("/api/admin/services/{id}/versions/{v2}"), def).await.unwrap();
    d.req("mark", "POST", &format!("/api/admin/services/{id}/versions/{v2}/publish"), json!({})).await.unwrap();
    let earlier = d.detail("new-resident", c).await.unwrap();
    assert_eq!(earlier["definition"], frozen["definition"]);
    assert_eq!(earlier["answers"], frozen["answers"]);
    assert_eq!(earlier["answers"]["consent"], true);
    assert_eq!(earlier["case"]["number"], frozen["case"]["number"]);
    assert!(
        earlier["timeline"].as_array().unwrap().iter().any(|e| e["summary"].as_str().unwrap().contains("received"))
    );
    assert_eq!(pdf(&mut d, "new-resident", &format!("/api/document-versions/{version}/download")).await, original_pdf);
    assert_eq!(d.req("new-resident","GET","/api/public/services/community-tree-advice",json!({})).await.unwrap()["definition"]["fields"].as_array().unwrap().len(),3);
}

#[tokio::test]
async fn building_v2_separate_approvals_modification_and_image_only_public_pdf() {
    let (mut d, dir) = support::fixture().await;
    let org = d.req("ben", "GET", "/api/my/organisations", json!({})).await.unwrap()[0]["id"].as_i64().unwrap();
    let (c, docs) = d
        .submit(
            "ben",
            "development-application",
            json!({"approvals_sought":["development_approval","building_approval"],"estimated_cost":120000}),
            Some(org),
        )
        .await
        .unwrap();
    d.action("olga", c, "advance").await.unwrap();
    scenarios::assess_fee(&mut d, c).await.unwrap();
    d.pay("ben", c, false).await.unwrap();
    let drawing = &docs["floor_plans"];
    let original = drawing["version_id"].as_i64().unwrap();
    let old_pdf = pdf(&mut d, "ben", &format!("/api/document-versions/{original}/download")).await;
    let revision = d.revision("priya", c).await.unwrap();
    let comment=d.req("priya","POST",&format!("/api/document-versions/{original}/comments"),json!({"expected_revision":revision,"body":"Replace A-101 with corrected dimensions.","visibility":"applicant","request_new_version":true})).await.unwrap()["id"].clone();
    d.req("ben", "POST", &format!("/api/cases/{c}/messages"), json!({"body":"I will replace it."})).await.unwrap();
    assert!(!d.detail("ben", c).await.unwrap()["required_action"].is_null());
    let source = servicehub::pdf::simple_document(
        "Fictional A-101 v2",
        &[],
        &[("Plan", "Corrected dimensions. Private phone: SECRET-PHONE-555-0199".into())],
    );
    let v2 = d
        .upload(
            "ben",
            &format!("/api/documents/{}/versions", drawing["id"]),
            &[("resolves_comment_ids", json!([comment]).to_string()), ("note", "Corrected A-101".into())],
            &source,
        )
        .await
        .unwrap()["version_id"]
        .as_i64()
        .unwrap();
    let detail = d.detail("ben", c).await.unwrap();
    assert!(detail["required_action"].is_null());
    assert!(detail["deadlines"].as_array().unwrap().iter().all(|v| v["status"] != "paused"));
    assert_eq!(pdf(&mut d, "ben", &format!("/api/document-versions/{original}/download")).await, old_pdf);
    scenarios::confirm_scope(&mut d, c, json!({"approvals":["development_approval","building_approval"]}))
        .await
        .unwrap();
    d.action("priya", c, "advance").await.unwrap();
    let now = d.state.now();
    let (exhibit, item) =
        scenarios::publish_exhibition(&mut d, c, v2, now - Duration::days(1), now + Duration::days(14)).await.unwrap();
    let public = pdf(&mut d, "stranger", &format!("/api/public/exhibitions/{exhibit}/items/{item}/file")).await;
    let source_path = dir.path().join("source.pdf");
    std::fs::write(&source_path, &source).unwrap();
    let source_text = tokio::process::Command::new("pdftotext")
        .arg(&source_path)
        .arg("-")
        .output()
        .await
        .expect("Poppler is required");
    assert!(source_text.status.success());
    assert!(String::from_utf8_lossy(&source_text.stdout).contains("SECRET-PHONE-555-0199"));
    let public_path = dir.path().join("public.pdf");
    std::fs::write(&public_path, &public).unwrap();
    let text = tokio::process::Command::new("pdftotext").arg(&public_path).arg("-").output().await.unwrap();
    assert!(text.status.success());
    assert!(String::from_utf8_lossy(&text.stdout).trim().is_empty());
    let fonts = tokio::process::Command::new("pdffonts").arg(&public_path).output().await.unwrap();
    assert!(fonts.status.success());
    assert_eq!(String::from_utf8_lossy(&fonts.stdout).lines().count(), 2, "Public PDF retains fonts/text layer");
    // The open exhibition (with a public comment) cannot be bypassed: no skip, no advance, no decision issue.
    scenarios::public_comment(&mut d, exhibit, "Fictional objection about the veranda height.").await.unwrap();
    let revision = d.revision("priya", c).await.unwrap();
    assert!(!d.detail("priya", c).await.unwrap()["allowed_actions"].as_array().unwrap().iter().any(|a| a == "skip"));
    d.expect(
        "priya",
        "POST",
        &format!("/api/cases/{c}/actions/skip"),
        json!({"expected_revision":revision,"reason":"Not needed"}),
        403,
    )
    .await
    .unwrap();
    d.expect("priya", "POST", &format!("/api/cases/{c}/actions/advance"), json!({"expected_revision":revision}), 409)
        .await
        .unwrap();
    let templates = d.req("priya", "GET", "/api/decision-templates", json!({})).await.unwrap();
    let template = templates.as_array().unwrap().iter().find(|t| t["decision_type"] == "development_approval").unwrap()
        ["id"]
        .clone();
    let development = d.req("priya","POST",&format!("/api/cases/{c}/decisions"),json!({"decision_type":"development_approval","outcome":"approved","reasons":"Fictional specialist assessment completed.","conditions":"","template_id":template,"evidence_version_ids":[v2],"expected_revision":revision})).await.unwrap()["id"].as_i64().unwrap();
    let revision = d.revision("priya", c).await.unwrap();
    d.req(
        "priya",
        "POST",
        &format!("/api/cases/{c}/decisions/{development}/submit"),
        json!({"expected_revision":revision}),
    )
    .await
    .unwrap();
    let revision = d.revision("helen", c).await.unwrap();
    d.expect(
        "helen",
        "POST",
        &format!("/api/cases/{c}/decisions/{development}/issue"),
        json!({"expected_revision":revision}),
        409,
    )
    .await
    .unwrap();
    // After the window closes the comment still needs a recorded consideration outcome.
    d.seed_time(now + Duration::days(15)).await.unwrap();
    let revision = d.revision("priya", c).await.unwrap();
    d.expect("priya", "POST", &format!("/api/cases/{c}/actions/advance"), json!({"expected_revision":revision}), 409)
        .await
        .unwrap();
    scenarios::consider_all(&mut d, exhibit, "Considered: height complies with the fictional plan control.")
        .await
        .unwrap();
    d.action("priya", c, "advance").await.unwrap();
    let revision = d.revision("helen", c).await.unwrap();
    d.req(
        "helen",
        "POST",
        &format!("/api/cases/{c}/decisions/{development}/issue"),
        json!({"expected_revision":revision}),
    )
    .await
    .unwrap();
    assert_ne!(d.detail("ben", c).await.unwrap()["case"]["status"], "completed");
    let building = d.decision(c, "building_approval", Some(vec![v2])).await.unwrap();
    assert_ne!(development, building);
    assert_eq!(d.detail("ben", c).await.unwrap()["case"]["status"], "completed");
    let decisions = d.req("ben", "GET", &format!("/api/cases/{c}/decisions"), json!({})).await.unwrap();
    for decision in decisions["items"].as_array().unwrap() {
        assert_eq!(decision["status"], "issued");
        assert_eq!(decision["evidence"].as_array().unwrap().len(), 1);
        assert_eq!(decision["evidence"][0]["id"], v2);
        assert_eq!(decision["evidence"][0]["version"], 2);
        pdf(&mut d, "ben", &format!("/api/document-versions/{}/download", decision["output_document_version_id"]))
            .await;
    }
    let (modification, _) = d
        .submit("ben", "modify-approval", json!({"original_approval":{"decision_id":building}}), Some(org))
        .await
        .unwrap();
    let approvals = d.req("ben", "GET", "/api/my/issued-approvals", json!({})).await.unwrap();
    let project =
        approvals.as_array().unwrap().iter().find(|v| v["id"] == building).unwrap()["project_id"].as_i64().unwrap();
    let linked = d.req("ben", "GET", &format!("/api/building-projects/{project}"), json!({})).await.unwrap();
    assert!(linked["cases"].as_array().unwrap().iter().any(|v| v["id"] == modification));
    assert!(
        linked["cases"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|c| c["links"].as_array().unwrap())
            .any(|v| v["kind"] == "modification_of")
    );
}

#[tokio::test]
async fn planning_certificate_requires_payment_and_specialist_assessment() {
    let (mut d, _dir) = support::fixture().await;
    let (c, _) =
        d.submit("alexey", "planning-certificate", json!({"sections":"Zoning and heritage"}), None).await.unwrap();
    d.action("olga", c, "advance").await.unwrap();
    let revision = d.revision("tom", c).await.unwrap();
    d.expect("tom", "POST", &format!("/api/cases/{c}/actions/advance"), json!({"expected_revision":revision}), 409)
        .await
        .unwrap();
    let templates = d.req("priya", "GET", "/api/decision-templates", json!({})).await.unwrap();
    let template = templates.as_array().unwrap().iter().find(|v| v["decision_type"] == "planning_certificate").unwrap();
    let revision = d.revision("priya", c).await.unwrap();
    let decision=d.req("priya","POST",&format!("/api/cases/{c}/decisions"),json!({"decision_type":"planning_certificate","outcome":"approved","reasons":"Checked","conditions":"Demo","template_id":template["id"],"expected_revision":revision})).await.unwrap()["id"].as_i64().unwrap();
    let revision = d.revision("priya", c).await.unwrap();
    d.req(
        "priya",
        "POST",
        &format!("/api/cases/{c}/decisions/{decision}/submit"),
        json!({"expected_revision":revision}),
    )
    .await
    .unwrap();
    let revision = d.revision("priya", c).await.unwrap();
    d.expect(
        "priya",
        "POST",
        &format!("/api/cases/{c}/decisions/{decision}/issue"),
        json!({"expected_revision":revision}),
        409,
    )
    .await
    .unwrap();
    d.pay("alexey", c, false).await.unwrap();
    assert_eq!(d.detail("alexey", c).await.unwrap()["case"]["current_step"], "preparation");
    let revision = d.revision("priya", c).await.unwrap();
    d.expect(
        "priya",
        "POST",
        &format!("/api/cases/{c}/decisions/{decision}/issue"),
        json!({"expected_revision":revision}),
        409,
    )
    .await
    .unwrap();
    d.action("priya", c, "advance").await.unwrap();
    let revision = d.revision("priya", c).await.unwrap();
    d.req(
        "helen",
        "POST",
        &format!("/api/cases/{c}/decisions/{decision}/issue"),
        json!({"expected_revision":revision}),
    )
    .await
    .unwrap();
    assert_eq!(d.detail("alexey", c).await.unwrap()["case"]["status"], "completed");
    let issued = d.req("alexey", "GET", &format!("/api/cases/{c}/decisions"), json!({})).await.unwrap();
    pdf(
        &mut d,
        "alexey",
        &format!("/api/document-versions/{}/download", issued["items"][0]["output_document_version_id"]),
    )
    .await;
}

#[tokio::test]
async fn hall_reschedule_preserves_history_and_refund_webhook_controls_completion() {
    let (mut d, _dir) = support::fixture().await;
    let hall = d.hall("rawson-whole", 21);
    let (c, _) = d.submit("ben", "rawson-hall-hire", hall, None).await.unwrap();
    confirm(&mut d, c, "ben").await;
    let old = d.req("ben", "GET", &format!("/api/cases/{c}/booking"), json!({})).await.unwrap()["booking"].clone();
    let old_path = format!("/api/cases/{c}/booking/confirmation/{}", old["confirmation_version_id"]);
    let old_pdf = pdf(&mut d, "ben", &old_path).await;
    let movement = json!({"unit_code":"rawson-main","start":d.instant(22,10,0),"end":d.instant(22,16,0),"reason":"Smaller celebration on the next day","expected_revision":old["revision"]});
    let preview =
        d.req("olga", "POST", &format!("/api/cases/{c}/booking/reschedule/preview"), movement.clone()).await.unwrap();
    assert_eq!(preview["available"], true);
    assert_eq!(preview["old_lines"][0]["amount_cents"], 15500);
    assert_eq!(preview["new_lines"][0]["amount_cents"], 11500);
    assert_eq!(
        d.req("olga", "POST", &format!("/api/cases/{c}/booking/reschedule"), movement).await.unwrap()["fees_changed"],
        true
    );
    let revised = d.req("ben", "GET", &format!("/api/cases/{c}/booking"), json!({})).await.unwrap();
    assert!(
        revised["history"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["revision"] == old["revision"] && v["start_at"] == old["start_at"])
    );
    assert_eq!(pdf(&mut d, "ben", &old_path).await, old_pdf);
    assert_ne!(revised["booking"]["confirmation_version_id"], old["confirmation_version_id"]);
    pdf(
        &mut d,
        "ben",
        &format!("/api/cases/{c}/booking/confirmation/{}", revised["booking"]["confirmation_version_id"]),
    )
    .await;
    let money = d.money("ben", c).await.unwrap();
    assert_eq!(money["customer_credit_cents"], 4000);
    assert_eq!(money["summary"]["settled"], true);
    assert_eq!(
        money["invoices"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["kind"] == "invoice")
            .flat_map(|i| i["lines"].as_array().unwrap())
            .filter(|l| l["kind"] == "deposit")
            .count(),
        1
    );
    d.complete_task(c, "venue_prep").await.unwrap();
    d.clock.advance(Duration::days(24));
    for who in ["ben", "olga", "jake", "tom"] {
        d.login(who).await.unwrap();
    }
    d.complete_task(c, "venue_inspection").await.unwrap();
    let money = d.money("tom", c).await.unwrap();
    assert_eq!(money["deposit_ready"], true);
    let bond = money["invoices"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "invoice")
        .flat_map(|i| i["lines"].as_array().unwrap())
        .find(|l| l["kind"] == "deposit")
        .unwrap();
    let revision = d.revision("tom", c).await.unwrap();
    let request = json!({"invoice_line_id":bond["id"],"refund_cents":20000,"retain_items":[{"label":"Extra cleaning","cents":5000}],"reason":"Inspection recorded extra cleaning","expected_revision":revision});
    let key = "bond-once";
    let (status, first) = d
        .raw(
            "tom",
            "POST",
            &format!("/api/cases/{c}/deposit-decision"),
            "application/json",
            request.to_string().into_bytes(),
            &[("Idempotency-Key", key.into())],
        )
        .await
        .unwrap();
    assert_eq!(status, 200);
    let (status, replay) = d
        .raw(
            "tom",
            "POST",
            &format!("/api/cases/{c}/deposit-decision"),
            "application/json",
            request.to_string().into_bytes(),
            &[("Idempotency-Key", key.into())],
        )
        .await
        .unwrap();
    assert_eq!(status, 200);
    assert_eq!(first, replay);
    d.drain().await.unwrap();
    let processing = d.money("ben", c).await.unwrap();
    assert_eq!(processing["refunds"][0]["status"], "processing");
    assert_eq!(processing["refunds"][0]["method"], "provider");
    assert_ne!(d.detail("ben", c).await.unwrap()["case"]["status"], "completed");
    let revision = d.revision("tom", c).await.unwrap();
    d.expect("tom", "POST", &format!("/api/cases/{c}/actions/advance"), json!({"expected_revision":revision}), 409)
        .await
        .unwrap();
    d.clock.advance(Duration::seconds(4));
    d.drain().await.unwrap();
    let final_money = d.money("ben", c).await.unwrap();
    assert_eq!(final_money["refunds"][0]["status"], "completed");
    assert_eq!(final_money["summary"]["deposits_held_cents"], 0);
    assert_eq!(d.detail("ben", c).await.unwrap()["case"]["status"], "completed");
}

#[tokio::test]
async fn equipment_final_invoice_uses_five_billable_hours_plus_expenses() {
    let (mut d, _dir) = support::fixture().await;
    let (c,_)=d.submit("ben","equipment-hire",json!({"request":{"description":"Bobcat","requested_hours":4,"preferred_date":"2026-10-09","site_text":"Fictional depot"}}),None).await.unwrap();
    let estimate = d.money("ben", c).await.unwrap();
    assert_eq!(estimate["invoices"][0]["kind"], "estimate");
    assert_eq!(estimate["invoices"][0]["total_cents"], 54000);
    d.action("olga", c, "advance").await.unwrap();
    let start = d.instant(2, 7, 30);
    let end = d.instant(2, 13, 0);
    scenarios::schedule_equipment(&mut d, c, &start, &end).await.unwrap();
    d.clock.advance(Duration::days(3));
    for who in ["ben", "olga", "jake", "tom"] {
        d.login(who).await.unwrap();
    }
    let usage = scenarios::finish_equipment(&mut d, c, &start, &end).await.unwrap();
    assert_eq!(usage["billable_minutes"], 300);
    let money = d.money("ben", c).await.unwrap();
    let invoices: Vec<_> = money["invoices"].as_array().unwrap().iter().filter(|i| i["kind"] == "invoice").collect();
    assert_eq!(invoices.len(), 1);
    let invoice = invoices[0];
    assert_eq!(invoice["total_cents"], 5 * 13500 + 1234);
    assert_eq!(invoice["lines"][0]["quantity_minutes"], 300);
    for text in ["Requested 4 h", "actual 5 h 30 min", "billable 5 h 0 min", "07:30", "13:00", "30 min downtime"] {
        assert!(invoice["basis_note"].as_str().unwrap().contains(text), "Missing {text}: {}", invoice["basis_note"]);
    }
    assert!(invoice["lines"][1]["description"].as_str().unwrap().contains("Agreed transport expenses"));
    pdf(&mut d, "ben", &format!("/api/document-versions/{}/download", invoice["document_version_id"])).await;
    d.pay("ben", c, false).await.unwrap();
    assert_eq!(d.detail("ben", c).await.unwrap()["case"]["status"], "completed");
}

#[tokio::test]
async fn concurrent_hall_confirmation_payment_replays_and_unclear_statement() {
    let (mut d, _dir) = support::fixture().await;
    let hall = d.hall("rawson-whole", 21);
    let (whole, _) = d.submit("alexey", "rawson-hall-hire", hall, None).await.unwrap();
    let hall = d.hall("rawson-main", 21);
    let (main, _) = d.submit("ben", "rawson-hall-hire", hall, None).await.unwrap();
    let mut session = String::new();
    for (c, who) in [(whole, "alexey"), (main, "ben")] {
        d.action("olga", c, "advance").await.unwrap();
        session = d.pay(who, c, true).await.unwrap();
    }
    let mut commands = Vec::new();
    for c in [whole, main] {
        let b = d.req("olga", "GET", &format!("/api/cases/{c}/booking"), json!({})).await.unwrap();
        let creds = &d.people["olga"];
        commands.push(
            Request::builder()
                .method("POST")
                .uri(format!("/api/cases/{c}/booking/confirm"))
                .header("Cookie", format!("nsh_session={}", creds.cookies["nsh_session"]))
                .header("X-CSRF-Token", &creds.csrf)
                .header("Content-Type", "application/json")
                .body(Body::from(json!({"expected_revision":b["booking"]["revision"]}).to_string()))
                .unwrap(),
        );
    }
    let second = commands.pop().unwrap();
    let first = commands.pop().unwrap();
    let (a, b) = tokio::join!(d.router.clone().oneshot(first), d.router.clone().oneshot(second));
    let mut statuses = [a.unwrap().status().as_u16(), b.unwrap().status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    let mut booking_statuses = Vec::new();
    for c in [whole, main] {
        booking_statuses.push(
            d.req("olga", "GET", &format!("/api/cases/{c}/booking"), json!({})).await.unwrap()["booking"]["status"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    booking_statuses.sort();
    assert_eq!(booking_statuses, vec!["confirmed", "requested"]);
    let (mut payload,): (String,) = sqlx::query_as(
        "SELECT payload_json FROM mock_pay_webhook_attempts WHERE json_extract(payload_json,'$.session_id')=?",
    )
    .bind(session)
    .fetch_one(&d.state.db)
    .await
    .unwrap();
    for changed_event in [false, true] {
        if changed_event {
            let mut event: Value = serde_json::from_str(&payload).unwrap();
            event["event_id"] = json!("another-event-same-payment");
            payload = event.to_string();
        }
        let signature =
            servicehub::mock::pay::signature(&d.state.cfg.webhook_secret, d.state.now().timestamp(), &payload);
        assert_eq!(
            d.raw(
                "provider",
                "POST",
                "/api/webhooks/demopay",
                "application/json",
                payload.clone().into_bytes(),
                &[("DemoPay-Signature", signature)]
            )
            .await
            .unwrap()
            .0,
            200
        );
    }
    for (c, who) in [(whole, "alexey"), (main, "ben")] {
        assert_eq!(d.money(who, c).await.unwrap()["payments"].as_array().unwrap().len(), 1);
    }
    let before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM payment_allocations").fetch_one(&d.state.db).await.unwrap();
    let csv = "date,amount,description,reference,bank_txn_id,payer\n2026-10-07,90.00,Unclear transfer,Unclear reference,ACCEPT-UNCLEAR,Fictional payer\n";
    let statement =
        d.req("tom", "POST", "/api/finance/statements", json!({"filename":"unclear.csv","csv":csv})).await.unwrap();
    assert_eq!(statement["rows"][0]["status"], "unmatched");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payment_allocations").fetch_one(&d.state.db).await.unwrap(),
        before
    );
    let suspense: i64 = sqlx::query_scalar(
        "SELECT SUM(credit_cents-debit_cents) FROM journal_lines WHERE account='unallocated_receipts'",
    )
    .fetch_one(&d.state.db)
    .await
    .unwrap();
    assert_eq!(suspense, 9000);
    scenarios::verify(&mut d).await.unwrap();
}

#[tokio::test]
async fn assisted_intake_response_and_confidential_complaint_isolation() {
    let (mut d, _dir) = support::fixture().await;
    let (_, answers) = d.answers("olga", "road-issue").await.unwrap();
    let c=d.req("olga","POST","/api/staff/intake",json!({"service":"road-issue","channel":"phone","applicant_name":"Fictional caller","applicant_email":"caller@example.invalid","applicant_phone":"+672355501","answers":answers})).await.unwrap()["id"].as_i64().unwrap();
    let applicant: Option<i64> = sqlx::query_scalar("SELECT applicant_user_id FROM cases WHERE id=?")
        .bind(c)
        .fetch_one(&d.state.db)
        .await
        .unwrap();
    assert!(applicant.is_none());
    d.action("olga", c, "advance").await.unwrap();
    d.complete_task(c, "road_inspection").await.unwrap();
    d.complete_task(c, "road_repair").await.unwrap();
    let revision = d.revision("olga", c).await.unwrap();
    d.req(
        "olga",
        "POST",
        &format!("/api/cases/{c}/road-response"),
        json!({"expected_revision":revision,"body":"Inspection complete; fictional pothole repaired."}),
    )
    .await
    .unwrap();
    d.drain().await.unwrap();
    assert_eq!(d.detail("olga", c).await.unwrap()["case"]["status"], "completed");
    let notifications = d.req("olga", "GET", "/api/demo/mailbox", json!({})).await.unwrap();
    assert!(
        notifications.as_array().unwrap().iter().any(|v| v["to"] == "caller@example.invalid" && v["status"] == "sent")
    );
    assert!(notifications.as_array().unwrap().iter().any(|v| v["to"] == "+672355501" && v["status"] == "sent"));
    let (complaint, _) = d.submit("alexey", "complaint", json!({}), None).await.unwrap();
    let source = servicehub::pdf::simple_document(
        "Sensitive complaint evidence",
        &[],
        &[("Evidence", "Private feedback about Olga".into())],
    );
    let upload = d
        .upload(
            "alexey",
            &format!("/api/cases/{complaint}/documents"),
            &[("title", "Confidential complaint attachment".into())],
            &source,
        )
        .await
        .unwrap();
    let vid = upload["version_id"].as_i64().unwrap();
    let revision = d.revision("ruth", complaint).await.unwrap();
    let olga = d.people["olga"].user_id;
    d.req(
        "ruth",
        "POST",
        &format!("/api/cases/{complaint}/complaint/subjects"),
        json!({"staff_user_ids":[olga],"expected_revision":revision}),
    )
    .await
    .unwrap();
    d.expect("olga", "GET", &format!("/api/cases/{complaint}"), json!({}), 404).await.unwrap();
    d.req("stranger", "GET", "/api/me", json!({})).await.unwrap();
    d.expect("stranger","POST","/api/auth/register",json!({"name":"Fictional stranger","email":"stranger@example.invalid","password":"Another-long-fictional-password-42!"}),201).await.unwrap();
    for who in ["olga", "ben", "stranger"] {
        assert_eq!(
            d.raw(who, "GET", &format!("/api/document-versions/{vid}/download"), "text/plain", vec![], &[])
                .await
                .unwrap()
                .0,
            404
        );
    }
    pdf(&mut d, "ruth", &format!("/api/document-versions/{vid}/download")).await;
    let number = d.detail("ruth", complaint).await.unwrap()["case"]["number"].as_str().unwrap().to_string();
    let search = d.req("olga", "GET", &format!("/api/staff/cases?q={number}"), json!({})).await.unwrap();
    assert!(search["items"].as_array().unwrap().is_empty());

    // Grant Olga a manager role through the admin API to exercise actual drill-down authorization,
    // while the explicit complaint-subject exclusion must still win.
    d.req("helen", "POST", &format!("/api/admin/users/{olga}/roles"), json!({"role":"manager"})).await.unwrap();
    let records = d.req("olga", "GET", &format!("/api/records/search?number={number}"), json!({})).await.unwrap();
    assert!(records.as_array().unwrap().is_empty());
    for metric in servicehub::records::metrics::METRICS {
        let rows = d.req("olga", "GET", &format!("/api/staff/dashboard/metrics/{metric}"), json!({})).await.unwrap();
        assert!(rows["items"].as_array().unwrap().iter().all(|v| v["id"] != complaint));
    }
    d.action("ruth", complaint, "advance").await.unwrap();
    d.action("ruth", complaint, "advance").await.unwrap();
    let revision = d.revision("ruth", complaint).await.unwrap();
    d.req("ruth","POST",&format!("/api/cases/{complaint}/letters"),json!({"letter_type":"complaint_response","title":"Complaint response","body":"Independent fictional response","expected_revision":revision})).await.unwrap();
    let records = d.req("olga", "GET", &format!("/api/records/search?number={number}"), json!({})).await.unwrap();
    assert!(records.as_array().unwrap().is_empty());
    let allowed = d.req("helen", "GET", &format!("/api/records/search?number={number}"), json!({})).await.unwrap();
    assert_eq!(allowed.as_array().unwrap().len(), 1);
    let review = d
        .req(
            "alexey",
            "POST",
            &format!("/api/cases/{complaint}/complaint/request-review"),
            json!({"reason":"Please review independently"}),
        )
        .await
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    d.expect("olga", "GET", &format!("/api/cases/{review}"), json!({}), 404).await.unwrap();
}

#[tokio::test]
async fn outage_retry_one_remote_record_backup_restore_and_dashboard_agreement() {
    let (mut d, dir) = support::fixture().await;
    d.req(
        "mark",
        "POST",
        "/api/admin/integrations/systems/content_manager",
        json!({"outage":true,"drop_responses":false}),
    )
    .await
    .unwrap();
    let (c, _) = d.submit("ben", "road-issue", json!({}), None).await.unwrap();
    d.action("olga", c, "cancel").await.unwrap();
    d.drain().await.unwrap();
    let failed = d.req("helen", "GET", &format!("/api/cases/{c}/integrations"), json!({})).await.unwrap();
    let delivery = failed.as_array().unwrap().iter().find(|v| v["kind"] == "record.case_closed").unwrap();
    assert_eq!(delivery["status"], "failed");
    assert!(!delivery["last_error"].is_null());
    let id = delivery["id"].as_i64().unwrap();
    let admin = d.req("mark", "GET", "/api/admin/integrations", json!({})).await.unwrap();
    let operation = admin.as_array().unwrap().iter().find(|v| v["id"] == id).unwrap()["operation_id"].clone();
    d.req(
        "mark",
        "POST",
        "/api/admin/integrations/systems/content_manager",
        json!({"outage":false,"drop_responses":false}),
    )
    .await
    .unwrap();
    d.req("mark", "POST", &format!("/api/admin/integrations/{id}/retry"), json!({})).await.unwrap();
    d.drain().await.unwrap();
    d.expect("mark", "POST", &format!("/api/admin/integrations/{id}/retry"), json!({}), 409).await.unwrap();
    let accepted = d.req("helen", "GET", &format!("/api/cases/{c}/integrations"), json!({})).await.unwrap();
    assert_eq!(accepted[0]["status"], "accepted");
    assert!(!accepted[0]["external_ref"].is_null());
    let remote = d.req("mark", "GET", "/api/admin/mock-records/content_manager", json!({})).await.unwrap();
    assert_eq!(remote.as_array().unwrap().iter().filter(|v| v["operation_id"] == operation).count(), 1);
    let (_, answers) = d.answers("olga", "road-issue").await.unwrap();
    let phone=d.req("olga","POST","/api/staff/intake",json!({"service":"road-issue","channel":"walk_in","applicant_name":"Fictional visitor","applicant_email":"visitor@example.invalid","answers":answers})).await.unwrap()["id"].as_i64().unwrap();
    d.action("olga", phone, "cancel").await.unwrap();
    d.action("helen", phone, "reopen").await.unwrap();
    scenarios::verify(&mut d).await.unwrap();
    let dashboard = d.req("helen", "GET", "/api/staff/dashboard", json!({})).await.unwrap();
    for service in dashboard["services"].as_array().unwrap() {
        for metric in servicehub::records::metrics::METRICS {
            let list = d
                .req(
                    "helen",
                    "GET",
                    &format!("/api/staff/dashboard/metrics/{metric}?service_id={}", service["service_id"]),
                    json!({}),
                )
                .await
                .unwrap();
            assert_eq!(service["metrics"][metric].as_u64().unwrap(), list["items"].as_array().unwrap().len() as u64);
        }
    }
    for person in dashboard["workload"].as_array().unwrap() {
        for metric in ["open", "overdue"] {
            let list = d
                .req(
                    "helen",
                    "GET",
                    &format!("/api/staff/dashboard/metrics/{metric}?owner_id={}", person["user_id"]),
                    json!({}),
                )
                .await
                .unwrap();
            assert_eq!(person[metric].as_u64().unwrap(), list["items"].as_array().unwrap().len() as u64);
        }
    }
    let export = d.raw("helen", "GET", &format!("/api/cases/{c}/export.zip"), "text/plain", vec![], &[]).await.unwrap();
    assert_eq!(export.0, 200);
    assert!(export.1.starts_with(b"PK"));
    // CLI acceptance uses the actual binary and temporary directories, including real blob inventory.
    let backup = dir.path().join("handoff-backup");
    for command in ["backup", "restore-check"] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_servicehub"))
            .arg(command)
            .arg(&backup)
            .env("DATA_DIR", &d.state.cfg.data_dir)
            .output()
            .await
            .unwrap();
        assert!(output.status.success(), "{command}: {}", String::from_utf8_lossy(&output.stderr));
    }
}
