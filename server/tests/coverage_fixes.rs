//! Acceptance review pass 2 regressions.
mod support;
use chrono::Duration;
use serde_json::{Value, json};
use servicehub::{AppState, clock, config::Config, db, seed::driver::Driver};
async fn req(d: &mut Driver, who: &str, method: &str, path: &str, body: Value) -> Value {
    d.req(who, method, path, body).await.unwrap()
}
async fn rev(d: &mut Driver, who: &str, c: i64) -> i64 {
    d.revision(who, c).await.unwrap()
}
async fn paid_hall(d: &mut Driver, who: &str, days: i64) -> i64 {
    let slot = d.hall("rawson-main", days);
    let (c, _) = d.submit(who, "rawson-hall-hire", slot, None).await.unwrap();
    d.action("olga", c, "advance").await.unwrap();
    d.pay(who, c, false).await.unwrap();
    let b = req(d, "olga", "GET", &format!("/api/cases/{c}/booking"), json!({})).await;
    req(
        d,
        "olga",
        "POST",
        &format!("/api/cases/{c}/booking/confirm"),
        json!({"expected_revision":b["booking"]["revision"]}),
    )
    .await;
    c
}
#[tokio::test]
async fn generic_result_shared_capabilities_reject_invalid_types_and_handlers() {
    let (mut d, _dir) = support::fixture().await;
    let caps = req(&mut d, "mark", "GET", "/api/admin/services/capabilities/generic", json!({})).await;
    assert_eq!(caps["decision_types"], json!(["service_response"]));
    assert!(!caps["handlers"].as_array().unwrap().contains(&json!("operations.booking_confirmed")));
    let created = req(
        &mut d,
        "mark",
        "POST",
        "/api/admin/services",
        json!({"slug":"new-result","name":"New result","category":"General","department":"Care","module":"generic"}),
    )
    .await;
    let base = format!("/api/admin/services/{}/versions/{}", created["id"], created["version_id"]);
    let mut def = json!({"module":"generic","summary":"Result service","outcome":"Written result","fields":[],"documents":[],"workflow":{"steps":[{"key":"decision","kind":"decision","role":"specialist","label":"Prepare result","applicant_label":"Preparing result","decision_types":["building_approval"]},{"key":"done","kind":"complete","label":"Complete","applicant_label":"Complete"}]},"deadlines":[],"pricing":[]});
    req(&mut d, "mark", "PUT", &base, def.clone()).await;
    assert!(
        !req(&mut d, "mark", "POST", &format!("{base}/validate"), json!({})).await["issues"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    d.expect("mark", "POST", &format!("{base}/publish"), json!({}), 422).await.unwrap();
    def["workflow"]["steps"][0] = json!({"key":"decision","kind":"module","handler":"operations.booking_confirmed","role":"specialist","label":"Bad","applicant_label":"Bad"});
    req(&mut d, "mark", "PUT", &base, def.clone()).await;
    assert!(
        !req(&mut d, "mark", "POST", &format!("{base}/validate"), json!({})).await["issues"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    def["workflow"]["steps"][0] = json!({"key":"job","kind":"task","task_kind":"equipment_job","role":"intake","label":"Invalid job","applicant_label":"Invalid job"});
    req(&mut d, "mark", "PUT", &base, def.clone()).await;
    assert!(
        !req(&mut d, "mark", "POST", &format!("{base}/validate"), json!({})).await["issues"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    def["workflow"]["steps"][0] = json!({"key":"decision","kind":"decision","decision_types":["service_response"],"role":"specialist","label":"Result","applicant_label":"Result"});
    req(&mut d, "mark", "PUT", &base, def).await;
    req(&mut d, "mark", "POST", &format!("{base}/publish"), json!({})).await;
    let (c, _) = d.submit("alexey", "new-result", json!({}), None).await.unwrap();
    let r = rev(&mut d, "priya", c).await;
    d.expect(
        "priya",
        "POST",
        &format!("/api/cases/{c}/actions/refuse"),
        json!({"expected_revision":r,"reason":"No"}),
        403,
    )
    .await
    .unwrap();
    d.decision(c, "service_response", None).await.unwrap();
    assert_eq!(d.detail("alexey", c).await.unwrap()["case"]["status"], "completed");
    let issued = req(&mut d, "alexey", "GET", &format!("/api/cases/{c}/decisions"), json!({})).await;
    let decision = &issued["items"][0];
    assert_ne!(decision["prepared_by"], decision["approved_by"]);
    let vid = decision["output_document_version_id"].as_i64().unwrap();
    let (status, bytes) = d
        .raw("alexey", "GET", &format!("/api/document-versions/{vid}/download"), "text/plain", vec![], &[])
        .await
        .unwrap();
    assert_eq!(status, 200);
    assert!(bytes.starts_with(b"%PDF-"));
}
#[tokio::test]
async fn complaint_subject_excluded_before_routing_including_only_officer_and_manager() {
    let (mut d, _dir) = support::fixture().await;
    let form = req(&mut d, "alexey", "GET", "/api/public/services/complaint", json!({})).await;
    let field =
        form["definition"]["fields"].as_array().unwrap().iter().find(|f| f["key"] == "staff_member_concerned").unwrap();
    assert_eq!(field["label"], "Staff member concerned (if known)");
    assert!(field["options"].as_array().unwrap().iter().all(|o| !o["label"].as_str().unwrap().contains('@')));
    for who in ["ruth", "helen"] {
        let uid = d.people[who].user_id;
        let (c, _) =
            d.submit("alexey", "complaint", json!({"staff_member_concerned":uid.to_string()}), None).await.unwrap();
        d.expect(who, "GET", &format!("/api/cases/{c}"), json!({}), 404).await.unwrap();
        let owner: i64 = sqlx::query_scalar(
            "SELECT user_id FROM case_assignments WHERE case_id=? AND role='owner' AND ended_at IS NULL",
        )
        .bind(c)
        .fetch_one(&d.state.db)
        .await
        .unwrap();
        assert_ne!(uid, owner);
        if who == "ruth" {
            assert_eq!(owner, d.people["helen"].user_id);
        }
    }
    let (c, _) = d.submit("alexey", "complaint", json!({}), None).await.unwrap();
    let r = rev(&mut d, "helen", c).await;
    let ruth = d.people["ruth"].user_id;
    req(
        &mut d,
        "helen",
        "POST",
        &format!("/api/cases/{c}/complaint/subjects"),
        json!({"staff_user_ids":[ruth],"expected_revision":r}),
    )
    .await;
    d.expect("ruth", "GET", &format!("/api/cases/{c}"), json!({}), 404).await.unwrap();
}
#[tokio::test]
async fn independent_decisions_and_role_escalation_are_enforced() {
    let (mut d, _dir) = support::fixture().await;
    let mark = d.people["mark"].user_id;
    let olga = d.people["olga"].user_id;
    d.expect("mark", "POST", &format!("/api/admin/users/{mark}/roles"), json!({"role":"manager"}), 403).await.unwrap();
    for role in ["manager", "complaints_officer"] {
        d.expect("mark", "POST", &format!("/api/admin/users/{olga}/roles"), json!({"role":role}), 403).await.unwrap();
    }
    req(&mut d, "helen", "POST", &format!("/api/admin/users/{olga}/roles"), json!({"role":"manager"})).await;
    let (c, _) = d.submit("alexey", "planning-certificate", json!({}), None).await.unwrap();
    d.action("olga", c, "advance").await.unwrap();
    d.pay("alexey", c, false).await.unwrap();
    d.action("priya", c, "advance").await.unwrap();
    let templates = req(&mut d, "priya", "GET", "/api/decision-templates", json!({})).await;
    let template = templates.as_array().unwrap().iter().find(|t| t["decision_type"] == "planning_certificate").unwrap()
        ["id"]
        .clone();
    let r = rev(&mut d, "priya", c).await;
    let id=req(&mut d,"priya","POST",&format!("/api/cases/{c}/decisions"),json!({"decision_type":"planning_certificate","outcome":"refused","reasons":"Information cannot be certified","template_id":template,"expected_revision":r})).await["id"].as_i64().unwrap();
    let r = rev(&mut d, "priya", c).await;
    req(&mut d, "priya", "POST", &format!("/api/cases/{c}/decisions/{id}/submit"), json!({"expected_revision":r}))
        .await;
    let r = rev(&mut d, "priya", c).await;
    d.expect("priya", "POST", &format!("/api/cases/{c}/decisions/{id}/issue"), json!({"expected_revision":r}), 403)
        .await
        .unwrap();
    req(&mut d, "helen", "POST", &format!("/api/cases/{c}/decisions/{id}/issue"), json!({"expected_revision":r})).await;
    assert_eq!(d.detail("alexey", c).await.unwrap()["case"]["status"], "refused");
}
#[tokio::test]
async fn unused_cancellation_refunds_fee_credit_and_bond_through_live_provider() {
    let (mut d, _dir) = support::fixture().await;
    let c = paid_hall(&mut d, "alexey", 10).await;
    let b = req(&mut d, "olga", "GET", &format!("/api/cases/{c}/booking"), json!({})).await;
    req(
        &mut d,
        "olga",
        "POST",
        &format!("/api/cases/{c}/booking/cancel"),
        json!({"reason":"Unused booking","expected_revision":b["booking"]["revision"]}),
    )
    .await;
    let money = d.money("tom", c).await.unwrap();
    let credit = money["customer_credit_cents"].as_i64().unwrap();
    assert!(credit > 0);
    let r = rev(&mut d, "tom", c).await;
    req(
        &mut d,
        "tom",
        "POST",
        &format!("/api/cases/{c}/refund-credit"),
        json!({"amount_cents":credit,"reason":"Unused hire fee","expected_revision":r}),
    )
    .await;
    let r = rev(&mut d, "tom", c).await;
    d.expect(
        "tom",
        "POST",
        &format!("/api/cases/{c}/refund-credit"),
        json!({"amount_cents":credit,"reason":"Double refund","expected_revision":r}),
        409,
    )
    .await
    .unwrap();
    let bond = money["invoices"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "invoice")
        .flat_map(|i| i["lines"].as_array().unwrap())
        .find(|l| l["kind"] == "deposit")
        .unwrap();
    req(&mut d,"tom","POST",&format!("/api/cases/{c}/deposit-decision"),json!({"invoice_line_id":bond["id"],"refund_cents":bond["paid_cents"],"retain_items":[],"reason":"Unused; full bond return","expected_revision":r})).await;
    assert!(
        d.money("tom", c).await.unwrap()["refunds"].as_array().unwrap().iter().all(|r| r["status"] == "processing")
    );
    d.drain().await.unwrap();
    d.clock.advance(Duration::seconds(4));
    d.drain().await.unwrap();
    let money = d.money("alexey", c).await.unwrap();
    assert!(money["refunds"].as_array().unwrap().iter().all(|r| r["status"] == "completed"));
    assert_eq!(money["summary"]["deposits_held_cents"], 0);
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM (SELECT entry_id FROM journal_lines GROUP BY entry_id HAVING SUM(debit_cents-credit_cents)<>0)").fetch_one(&d.state.db).await.unwrap(),0);
}
#[tokio::test]
async fn wrong_applicant_bank_match_can_return_to_suspense_and_rematch() {
    let (mut d, _dir) = support::fixture().await;
    let (a, _) = d.submit("alexey", "planning-certificate", json!({}), None).await.unwrap();
    d.action("olga", a, "advance").await.unwrap();
    let (b, _) = d.submit("ben", "planning-certificate", json!({}), None).await.unwrap();
    d.action("olga", b, "advance").await.unwrap();
    let number = d.detail("alexey", a).await.unwrap()["case"]["number"].as_str().unwrap().to_owned();
    let imported=req(&mut d,"tom","POST","/api/finance/statements",json!({"filename":"wrong.csv","csv":format!("date,amount,description,reference,bank_txn_id,payer\n2026-10-07,181.13,Transfer,{number},wrong-customer,Fictional payer\n")})).await;
    let row = &imported["rows"][0];
    assert_eq!(row["status"], "matched");
    let payment = sqlx::query_scalar::<_, i64>("SELECT payment_id FROM statement_rows WHERE id=?")
        .bind(row["id"].as_i64().unwrap())
        .fetch_one(&d.state.db)
        .await
        .unwrap();
    let rowid = row["id"].as_i64().unwrap();
    let r = rev(&mut d, "tom", a).await;
    d.expect(
        "tom",
        "POST",
        &format!("/api/finance/payments/{payment}/unmatch"),
        json!({"reason":"Wrong applicant","expected_revision":r}),
        204,
    )
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT suspense_cents FROM finance_payment_balances WHERE payment_id=?")
            .bind(payment)
            .fetch_one(&d.state.db)
            .await
            .unwrap(),
        18113
    );
    let r = rev(&mut d, "tom", b).await;
    d.expect(
        "tom",
        "POST",
        &format!("/api/finance/statement-rows/{rowid}/match"),
        json!({"case_id":b,"expected_revision":r}),
        204,
    )
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, Option<i64>>("SELECT case_id FROM payments WHERE id=?")
            .bind(payment)
            .fetch_one(&d.state.db)
            .await
            .unwrap(),
        Some(b)
    );
    let old = d.money("alexey", a).await.unwrap();
    assert_eq!(old["summary"]["paid_cents"], 0);
    assert!(old["payments"].as_array().unwrap().is_empty());
    assert_eq!(d.money("ben", b).await.unwrap()["summary"]["paid_cents"], 18113);
}
#[tokio::test]
async fn manager_waivers_apply_to_venue_and_definition_invoices_with_reason() {
    let (mut d, _dir) = support::fixture().await;
    for slug in ["planning-certificate", "rawson-hall-hire"] {
        let answers = if slug == "rawson-hall-hire" { d.hall("rawson-main", 13) } else { json!({}) };
        let (c, _) = d.submit("alexey", slug, answers, None).await.unwrap();
        let code = if slug == "rawson-hall-hire" { "HALL_MAIN_DAY" } else { "PLANNING_CERT" };
        let r = rev(&mut d, "helen", c).await;
        d.expect(
            "tom",
            "POST",
            &format!("/api/cases/{c}/price-waivers"),
            json!({"item_code":code,"amount_cents":1000,"reason":"Community exemption","expected_revision":r}),
            403,
        )
        .await
        .unwrap();
        d.expect(
            "helen",
            "POST",
            &format!("/api/cases/{c}/price-waivers"),
            json!({"item_code":code,"amount_cents":1000,"reason":"Community exemption","expected_revision":r}),
            201,
        )
        .await
        .unwrap();
        d.action("olga", c, "advance").await.unwrap();
        let money = d.money("alexey", c).await.unwrap();
        assert_eq!(money["invoices"][0]["total_cents"], if slug == "rawson-hall-hire" { 35500 } else { 17113 });
        assert!(money["invoices"][0]["lines"].as_array().unwrap().iter().any(|l| {
            l["description"].as_str().unwrap().contains("Waiver of $10.00: Community exemption. Approved by Helen")
        }));
        d.pay("alexey", c, false).await.unwrap();
        if slug == "rawson-hall-hire" {
            let b = req(&mut d, "olga", "GET", &format!("/api/cases/{c}/booking"), json!({})).await;
            req(
                &mut d,
                "olga",
                "POST",
                &format!("/api/cases/{c}/booking/confirm"),
                json!({"expected_revision":b["booking"]["revision"]}),
            )
            .await;
        }
    }
}
#[tokio::test]
async fn receipt_type_check_search_deadlines_team_copies_and_organisation_creation() {
    let (mut d, _dir) = support::fixture().await;
    for (q, module) in [("birthday%20party", "venue_booking"), ("hole%20in%20the%20road", "road_issue")] {
        assert!(
            req(&mut d, "alexey", "GET", &format!("/api/public/services?q={q}"), json!({})).await["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["module"] == module)
        );
    }
    req(&mut d, "mark", "POST", "/api/admin/services/synonyms", json!({"token":"bash","replacement":"hall"})).await;
    assert!(
        req(&mut d, "alexey", "GET", "/api/public/services?q=bash", json!({})).await["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["module"] == "venue_booking")
    );
    req(
        &mut d,
        "mark",
        "POST",
        "/api/admin/settings",
        json!({"notify.customer_care_email":"new-care@example.invalid"}),
    )
    .await;
    let (c, _) = d.submit("alexey", "planning-certificate", json!({}), None).await.unwrap();
    let bytes = servicehub::pdf::simple_document("Receipt evidence", &[], &[]);
    d.upload(
        "alexey",
        &format!("/api/cases/{c}/documents"),
        &[("category", "receipt".into()), ("title", "Proof of payment".into())],
        &bytes,
    )
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payments WHERE case_id=?")
            .bind(c)
            .fetch_one(&d.state.db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(sqlx::query_scalar::<_,String>("SELECT b.scan_status FROM blobs b JOIN document_versions v ON v.blob_id=b.id JOIN documents d ON d.id=v.document_id WHERE d.case_id=? AND d.category='receipt'").bind(c).fetch_one(&d.state.db).await.unwrap(),"not_scanned");
    let r = rev(&mut d, "olga", c).await;
    let uid = d.people["priya"].user_id;
    req(
        &mut d,
        "olga",
        "POST",
        &format!("/api/cases/{c}/assign"),
        json!({"user_id":uid,"role":"collaborator","reason":"Care copy","expected_revision":r}),
    )
    .await;
    assert!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM notifications WHERE case_id=? AND to_address='new-care@example.invalid'"
        )
        .bind(c)
        .fetch_one(&d.state.db)
        .await
        .unwrap()
            > 0
    );
    assert!(
        d.detail("alexey", c).await.unwrap()["deadlines"]
            .as_array()
            .unwrap()
            .iter()
            .all(|dl| !dl["text"].as_str().unwrap().starts_with("We will reply by"))
    );
    assert_eq!(
        servicehub::deadlines::api::resident_text("Completeness check", "met", "2026-10-10T00:00:00Z", 0, None)
            .unwrap(),
        "Completeness check: completed."
    );
    let org = req(&mut d, "alexey", "POST", "/api/my/organisations", json!({"name":"Island business","abn":"123"}))
        .await["id"]
        .clone();
    let list = req(&mut d, "alexey", "GET", "/api/my/organisations", json!({})).await;
    assert!(list.as_array().unwrap().iter().any(|o| o["id"] == org && o["role"] == "owner"));
}
#[tokio::test]
async fn assignment_collaboration_replace_end_escalate_uses_injected_time() {
    let (mut d, _dir) = support::fixture().await;
    let (c, _) = d.submit("alexey", "planning-certificate", json!({}), None).await.unwrap();
    d.seed_time(d.state.now() + Duration::days(3)).await.unwrap();
    let priya = d.people["priya"].user_id;
    let helen = d.people["helen"].user_id;
    for (uid, role, replace) in [(priya, "collaborator", false), (helen, "owner", true)] {
        let r = rev(&mut d, "olga", c).await;
        req(
            &mut d,
            "olga",
            "POST",
            &format!("/api/cases/{c}/assign"),
            json!({"user_id":uid,"role":role,"reason":"Handover","replace_owner":replace,"expected_revision":r}),
        )
        .await;
    }
    let history = req(&mut d, "olga", "GET", &format!("/api/cases/{c}/assignments"), json!({})).await;
    let collaborator = history.as_array().unwrap().iter().find(|a| a["role"] == "collaborator").unwrap();
    assert_eq!(servicehub::time::parse(collaborator["assigned_at"].as_str().unwrap()).unwrap(), d.state.now());
    let aid = collaborator["id"].clone();
    let r = rev(&mut d, "olga", c).await;
    req(
        &mut d,
        "olga",
        "POST",
        &format!("/api/cases/{c}/assignments/{aid}/end"),
        json!({"reason":"Finished","expected_revision":r}),
    )
    .await;
    let r = rev(&mut d, "olga", c).await;
    req(
        &mut d,
        "olga",
        "POST",
        &format!("/api/cases/{c}/escalate"),
        json!({"reason":"Manager review","expected_revision":r}),
    )
    .await;
    assert!(
        req(&mut d, "olga", "GET", &format!("/api/cases/{c}/assignments"), json!({}))
            .await
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["user_id"] == helen && a["role"] == "collaborator")
    );
}
#[tokio::test]
async fn accountless_letter_contains_answer_in_email_and_sms() {
    let (mut d, _dir) = support::fixture().await;
    let (_, answers) = d.answers("olga", "road-issue").await.unwrap();
    let c=req(&mut d,"olga","POST","/api/staff/intake",json!({"service":"road-issue","channel":"phone","applicant_name":"Accountless caller","applicant_email":"caller@example.invalid","applicant_phone":"+672355512","answers":answers})).await["id"].as_i64().unwrap();
    // A response letter is accepted only at the response step.
    d.action("olga", c, "advance").await.unwrap();
    d.complete_task(c, "road_inspection").await.unwrap();
    d.complete_task(c, "road_repair").await.unwrap();
    let r = rev(&mut d, "olga", c).await;
    req(&mut d,"olga","POST",&format!("/api/cases/{c}/letters"),json!({"letter_type":"road_response","title":"Your road answer","body":"The drain was inspected and cleared today.","expected_revision":r})).await;
    let rows:Vec<(String,String)>=sqlx::query_as("SELECT channel,body FROM notifications WHERE case_id=? AND subject LIKE '%response%' AND channel IN ('email','sms')").bind(c).fetch_all(&d.state.db).await.unwrap();
    for channel in ["email", "sms"] {
        assert!(
            rows.iter().any(|(ch, body)| ch == channel && body.contains("The drain was inspected and cleared today."))
        );
    }
}
#[tokio::test]
async fn bootstrap_catalogue_non_demo_mocks_and_cross_directory_restore() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::for_tests(dir.path());
    cfg.demo_mode = false;
    let state = AppState::new(cfg).await.unwrap();
    db::migrate(&state.db).await.unwrap();
    servicehub::bootstrap::seed_catalogue(&state).await.unwrap();
    servicehub::bootstrap::seed_catalogue(&state).await.unwrap();
    for table in ["users", "cases", "submissions"] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}")).fetch_one(&state.db).await.unwrap(),
            0
        );
    }
    let password =
        servicehub::bootstrap::create_admin(&state, "admin@example.invalid", "First administrator").await.unwrap();
    let (hash, enabled, change): (String, bool, bool) =
        sqlx::query_as("SELECT password_hash,totp_enabled,must_change_password FROM users")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert!(servicehub::auth::password::verify(&password, &hash));
    assert!(!enabled);
    assert!(change);
    let mut d = Driver::new(state.clone(), state.now()).await.unwrap();
    req(&mut d, "bootstrap", "GET", "/api/me", json!({})).await;
    assert_eq!(
        req(
            &mut d,
            "bootstrap",
            "POST",
            "/api/auth/login",
            json!({"email":"admin@example.invalid","password":password})
        )
        .await["mfa_required"],
        true
    );
    assert_eq!(
        d.raw("bootstrap", "GET", "/mock/pay/checkout/missing", "text/plain", vec![], &[]).await.unwrap().0,
        404
    );
    d.expect("bootstrap", "GET", "/api/admin/users", json!({}), 401).await.unwrap();
    let (mut demo, source) = support::fixture().await;
    let c = paid_hall(&mut demo, "alexey", 12).await;
    let invoice = demo.money("alexey", c).await.unwrap()["invoices"][0]["document_version_id"].as_i64().unwrap();
    let original = demo
        .raw("alexey", "GET", &format!("/api/document-versions/{invoice}/download"), "text/plain", vec![], &[])
        .await
        .unwrap()
        .1;
    let before = req(&mut demo, "helen", "GET", "/api/staff/dashboard", json!({})).await;
    let backup = source.path().join("backup");
    servicehub::records::backup::backup(&demo.state, &backup).await.unwrap();
    let fresh = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new(env!("CARGO_BIN_EXE_servicehub"))
            .arg("restore-check")
            .arg(&backup)
            .env("DATA_DIR", fresh.path())
            .env("DEMO_MODE", "false")
            .env("DEMO_ENDS_AT", "")
            .status()
            .unwrap()
            .success()
    );
    let restored = tempfile::tempdir().unwrap();
    std::fs::copy(backup.join("servicehub.db"), restored.path().join("servicehub.db")).unwrap();
    fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            let target = to.join(e.file_name());
            if e.path().is_dir() {
                copy_dir(&e.path(), &target)
            } else {
                std::fs::copy(e.path(), target).unwrap();
            }
        }
    }
    copy_dir(&backup.join("blobs"), &restored.path().join("blobs"));
    let restored_state =
        AppState::with_clock(Config::for_tests(restored.path()), demo.state.clock.clone()).await.unwrap();
    db::migrate(&restored_state.db).await.unwrap();
    let mut other = Driver::new(restored_state, demo.state.now() + Duration::seconds(31)).await.unwrap();
    other.login("helen").await.unwrap();
    other.login("alexey").await.unwrap();
    assert_eq!(before["metrics"], req(&mut other, "helen", "GET", "/api/staff/dashboard", json!({})).await["metrics"]);
    assert_eq!(
        other
            .raw("alexey", "GET", &format!("/api/document-versions/{invoice}/download"), "text/plain", vec![], &[])
            .await
            .unwrap()
            .1,
        original
    );
    let mut cfg = (*demo.state.cfg).clone();
    cfg.demo_mode = false;
    let non_demo = AppState { cfg: std::sync::Arc::new(cfg), ..demo.state.clone() };
    let mut disabled = Driver::new(non_demo, demo.state.now()).await.unwrap();
    disabled.people = demo.people.clone();
    assert_eq!(req(&mut disabled, "mark", "GET", "/api/me", json!({})).await["ai_enabled"], false);
    let money = req(&mut disabled, "alexey", "GET", &format!("/api/cases/{c}/money"), json!({})).await;
    assert_eq!(money["online_payment_enabled"], false);
    assert!(
        disabled
            .expect(
                "alexey",
                "POST",
                &format!("/api/cases/{c}/checkout"),
                json!({"invoice_id":money["invoices"][0]["id"]}),
                409
            )
            .await
            .unwrap()
            .to_string()
            .contains("Online payment is not configured")
    );
    assert_eq!(
        disabled.raw("alexey", "POST", "/mock/mail/send", "application/json", b"{}".to_vec(), &[]).await.unwrap().0,
        404
    );
    let newdir = tempfile::tempdir().unwrap();
    let fresh_demo = AppState::new(Config::for_tests(newdir.path())).await.unwrap();
    db::migrate(&fresh_demo.db).await.unwrap();
    clock::scope(fresh_demo.clock.clone(), servicehub::bootstrap::seed_fresh_demo(&fresh_demo)).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cases").fetch_one(&fresh_demo.db).await.unwrap();
    assert!(count > 0);
    servicehub::bootstrap::seed_fresh_demo(&fresh_demo).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM cases").fetch_one(&fresh_demo.db).await.unwrap(),
        count
    );
}

async fn multipart(d: &mut Driver, path: &str, parts: &[(&str, &str, &[u8])]) -> Value {
    let boundary = "coverage-originals";
    let mut body = Vec::new();
    for (key, name, bytes) in parts {
        body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{key}\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
        body.extend(*bytes);
        body.extend(b"\r\n");
    }
    body.extend(format!("--{boundary}--\r\n").as_bytes());
    let (status, bytes) =
        d.raw("mark", "POST", path, &format!("multipart/form-data; boundary={boundary}"), body, &[]).await.unwrap();
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice(&bytes).unwrap()
}
#[tokio::test]
async fn bulk_forms_keep_distinct_originals_and_enforce_source_access() {
    let (mut d, _dir) = support::fixture().await;
    let def = json!({"module":"generic","summary":"Original form","outcome":"Response","fields":[],"documents":[],"workflow":{"steps":[{"key":"done","kind":"complete","label":"Done","applicant_label":"Done"}]},"deadlines":[],"pricing":[]});
    let items = json!([{"slug":"form-a","name":"Form A","module":"generic","category":"General","department":"Care","source_file":"a.pdf","definition":def},{"slug":"form-b","name":"Form B","module":"generic","category":"General","department":"Care","source_file":"b.pdf","definition":def}]);
    let bytes = serde_json::to_vec(&items).unwrap();
    let a = servicehub::pdf::simple_document("Original A", &[], &[]);
    let b = servicehub::pdf::simple_document("Original B", &[], &[]);
    let uploaded = multipart(
        &mut d,
        "/api/admin/service-imports",
        &[("file", "forms.json", &bytes), ("source", "a.pdf", &a), ("source", "b.pdf", &b)],
    )
    .await;
    assert_eq!(uploaded["status"], "validated");
    servicehub::storage::gc_older_than(&d.state, std::time::Duration::ZERO).await.unwrap();
    let applied =
        req(&mut d, "mark", "POST", &format!("/api/admin/service-imports/{}/apply", uploaded["id"]), json!({})).await;
    for (row, expected) in applied["report"]["items"].as_array().unwrap().iter().zip([a, b]) {
        let path = format!("/api/admin/services/{}/versions/{}/source", row["service_id"], row["version_id"]);
        let (status, actual) = d.raw("mark", "GET", &path, "text/plain", vec![], &[]).await.unwrap();
        assert_eq!(status, 200);
        assert_eq!(actual, expected);
        d.expect("alexey", "GET", &path, json!({}), 403).await.unwrap();
    }
    let missing = multipart(&mut d, "/api/admin/service-imports", &[("file", "forms.json", &bytes)]).await;
    assert_eq!(missing["status"], "has_errors");
}
fn unzip_stored(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut entries = std::collections::BTreeMap::new();
    let mut offset = 0;
    while bytes.get(offset..offset + 4) == Some(b"PK\x03\x04") {
        let n = u16::from_le_bytes(bytes[offset + 26..offset + 28].try_into().unwrap()) as usize;
        let extra = u16::from_le_bytes(bytes[offset + 28..offset + 30].try_into().unwrap()) as usize;
        let size = u32::from_le_bytes(bytes[offset + 18..offset + 22].try_into().unwrap()) as usize;
        let name = String::from_utf8(bytes[offset + 30..offset + 30 + n].to_vec()).unwrap();
        let start = offset + 30 + n + extra;
        entries.insert(name, bytes[start..start + size].to_vec());
        offset = start + size;
    }
    entries
}
#[tokio::test]
async fn legacy_documents_complete_export_and_configurable_endpoints() {
    let (mut d, _dir) = support::fixture().await;
    let csv = "source_system,source_id,service_slug,applicant_name,applicant_email,property_ref,title,opened_on,closed_on,status,notes\nold,1,road-issue,Former caller,former@example.invalid,Lot 1,Historic drain,2020-01-01,2020-02-01,completed,Original record\n";
    let batch = req(&mut d, "mark", "POST", "/api/admin/legacy-imports", json!({"filename":"old.csv","csv":csv})).await;
    let original = servicehub::pdf::simple_document("Original historical record", &[], &[]);
    d.upload(
        "mark",
        &format!("/api/admin/legacy-imports/{}/documents", batch["id"]),
        &[("source_system", "old".into()), ("source_id", "1".into()), ("title", "Historical PDF".into())],
        &original,
    )
    .await
    .unwrap();
    let imported = req(
        &mut d,
        "mark",
        "POST",
        &format!("/api/admin/legacy-imports/{}/import", batch["id"]),
        json!({"skip_possible_duplicates":false}),
    )
    .await;
    let c = imported["rows"][0]["case_id"].as_i64().unwrap();
    let (status, bytes) =
        d.raw("helen", "GET", &format!("/api/cases/{c}/export.zip"), "text/plain", vec![], &[]).await.unwrap();
    assert_eq!(status, 200);
    let files = unzip_stored(&bytes);
    assert!(files.values().any(|b| *b == original));
    let data: Value = serde_json::from_slice(&files["case.json"]).unwrap();
    assert_eq!(data["case"]["id"], c);
    assert!(!data["documents"].as_array().unwrap().is_empty());
    for key in ["bookings", "payments", "invoices", "invoice_lines", "payment_allocations", "tasks", "deadlines"] {
        assert!(data[key].is_array(), "{key}");
    }
    let paid = paid_hall(&mut d, "alexey", 14).await;
    let (_, bytes) =
        d.raw("helen", "GET", &format!("/api/cases/{paid}/export.zip"), "text/plain", vec![], &[]).await.unwrap();
    let files = unzip_stored(&bytes);
    let data: Value = serde_json::from_slice(&files["case.json"]).unwrap();
    for key in ["bookings", "payments", "invoices", "invoice_lines", "payment_allocations", "tasks", "deadlines"] {
        assert!(!data[key].as_array().unwrap().is_empty(), "{key}");
    }
    let invoice = d.money("alexey", paid).await.unwrap()["invoices"][0]["document_version_id"].as_i64().unwrap();
    let (_, original) = d
        .raw("alexey", "GET", &format!("/api/document-versions/{invoice}/download"), "text/plain", vec![], &[])
        .await
        .unwrap();
    assert!(files.values().any(|bytes| *bytes == original));
    req(
        &mut d,
        "mark",
        "POST",
        "/api/admin/integrations/systems/content_manager",
        json!({"enabled":true,"outage":false,"drop_responses":false,"base_url":"https://records.example.invalid/api"}),
    )
    .await;
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT base_url FROM external_systems WHERE code='content_manager'")
            .fetch_one(&d.state.db)
            .await
            .unwrap(),
        "https://records.example.invalid/api"
    );
}
#[tokio::test]
async fn exhibition_approver_sees_redaction_nsh_lookup_and_manager_withdraws() {
    let (mut d, _dir) = support::fixture().await;
    let (c, docs) = d.submit("alexey", "development-application", json!({}), None).await.unwrap();
    let number = d.detail("priya", c).await.unwrap()["case"]["number"].as_str().unwrap().to_owned();
    let lookup = req(&mut d, "priya", "GET", &format!("/api/exhibitions/lookup/{number}"), json!({})).await;
    assert_eq!(lookup["id"], c);
    let source = docs["floor_plans"]["version_id"].as_i64().unwrap();
    let now = d.state.now();
    let (e, item) = servicehub::seed::scenarios::publish_exhibition(
        &mut d,
        c,
        source,
        now - Duration::days(1),
        now + Duration::days(2),
    )
    .await
    .unwrap();
    let (_, plain) = d
        .raw("helen", "GET", &format!("/api/exhibitions/{e}/items/{item}/pages/1.png"), "text/plain", vec![], &[])
        .await
        .unwrap();
    let (status, redacted) = d
        .raw(
            "helen",
            "GET",
            &format!("/api/exhibitions/{e}/items/{item}/redacted-pages/1.png"),
            "text/plain",
            vec![],
            &[],
        )
        .await
        .unwrap();
    assert_eq!(status, 200);
    let a = printpdf::image_crate::load_from_memory(&plain).unwrap().to_rgb8();
    let b = printpdf::image_crate::load_from_memory(&redacted).unwrap().to_rgb8();
    assert_ne!(a, b);
    let (x, y) = (b.width() / 2, b.height() / 4);
    assert!(b.get_pixel(x, y).0.iter().all(|p| *p < 10));
    let r = rev(&mut d, "priya", c).await;
    let withdrawal = json!({"reason":"Published copy must be taken down.","expected_revision":r});
    d.expect("priya", "POST", &format!("/api/exhibitions/{e}/withdraw"), withdrawal.clone(), 403).await.unwrap();
    req(&mut d, "helen", "POST", &format!("/api/exhibitions/{e}/withdraw"), withdrawal).await;
    d.expect("stranger", "GET", &format!("/api/public/exhibitions/{e}"), json!({}), 404).await.unwrap();
    assert_eq!(
        d.raw("stranger", "GET", &format!("/api/public/exhibitions/{e}/items/{item}/file"), "text/plain", vec![], &[])
            .await
            .unwrap()
            .0,
        404
    );
    assert_eq!(
        sqlx::query_scalar::<_, Option<i64>>("SELECT published_blob_id FROM exhibition_items WHERE id=?")
            .bind(item)
            .fetch_one(&d.state.db)
            .await
            .unwrap(),
        None
    );
}
#[tokio::test]
async fn resident_reschedule_preview_includes_lines_and_credit_delta() {
    let (mut d, _dir) = support::fixture().await;
    let c = paid_hall(&mut d, "alexey", 20).await;
    let mut slot = d.hall("rawson-supper", 22)["slot"].clone();
    slot["reason"] = json!("Compare alternatives");
    slot["expected_revision"] =
        req(&mut d, "alexey", "GET", &format!("/api/cases/{c}/booking"), json!({})).await["booking"]["revision"]
            .clone();
    let result =
        req(&mut d, "alexey", "POST", &format!("/api/cases/{c}/booking/reschedule/preview"), slot.clone()).await;
    assert!(result["available"].as_bool().unwrap());
    assert!(!result["old_lines"].as_array().unwrap().is_empty());
    assert!(!result["new_lines"].as_array().unwrap().is_empty());
    assert!(result["credit_delta_cents"].as_i64().unwrap() > 0);
    d.expect("alexey", "POST", &format!("/api/cases/{c}/booking/reschedule"), slot, 403).await.unwrap();
}
#[tokio::test]
async fn document_class_retention_and_duplicate_reopened_periods() {
    let (mut d, _dir) = support::fixture().await;
    req(
        &mut d,
        "mark",
        "POST",
        "/api/admin/retention-rules",
        json!({"record_class":"document:letter","retain_years":12,"description":"Longer response record"}),
    )
    .await;
    let (c, _) = d.submit("alexey", "road-issue", json!({}), None).await.unwrap();
    d.action("olga", c, "advance").await.unwrap();
    d.complete_task(c, "road_inspection").await.unwrap();
    d.complete_task(c, "road_repair").await.unwrap();
    let r = rev(&mut d, "olga", c).await;
    req(
        &mut d,
        "olga",
        "POST",
        &format!("/api/cases/{c}/letters"),
        json!({"letter_type":"road_response","title":"Closed answer","body":"Resolved","expected_revision":r}),
    )
    .await;
    // The letter is issued at the response step and completes (closes) the case.
    assert_eq!(d.detail("olga", c).await.unwrap()["case"]["status"], "completed");
    let until: String =
        sqlx::query_scalar("SELECT retention_until FROM documents WHERE case_id=? AND category='letter'")
            .bind(c)
            .fetch_one(&d.state.db)
            .await
            .unwrap();
    assert_eq!(until, "2038-10-07");
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT retention_until FROM cases WHERE id=?")
            .bind(c)
            .fetch_one(&d.state.db)
            .await
            .unwrap(),
        until
    );
    d.seed_time(d.state.now() + Duration::days(3)).await.unwrap();
    let r = rev(&mut d, "helen", c).await;
    req(
        &mut d,
        "helen",
        "POST",
        &format!("/api/cases/{c}/actions/reopen"),
        json!({"reason":"Further investigation","expected_revision":r}),
    )
    .await;
    for (day, n) in [("2026-10-07", 0), ("2026-10-10", 1)] {
        let drill = req(
            &mut d,
            "helen",
            "GET",
            &format!("/api/staff/dashboard/metrics/reopened?from={day}&to={day}"),
            json!({}),
        )
        .await;
        assert_eq!(drill["items"].as_array().unwrap().len(), n);
    }
    let (other, _) = d.submit("ben", "road-issue", json!({}), None).await.unwrap();
    let r = rev(&mut d, "olga", other).await;
    req(
        &mut d,
        "olga",
        "POST",
        &format!("/api/cases/{other}/actions/close-duplicate"),
        json!({"of_case":c,"reason":"Same drain","expected_revision":r}),
    )
    .await;
    let dashboard = req(&mut d, "helen", "GET", "/api/staff/dashboard?from=2026-10-10&to=2026-10-10", json!({})).await;
    let drill = req(
        &mut d,
        "helen",
        "GET",
        "/api/staff/dashboard/metrics/closed_duplicate?from=2026-10-10&to=2026-10-10",
        json!({}),
    )
    .await;
    assert_eq!(dashboard["metrics"]["closed_duplicate"], 1);
    assert_eq!(drill["items"][0]["id"], other);
}
#[tokio::test]
async fn account_recovery_forces_totp_and_password_rotation() {
    let (mut d, _dir) = support::fixture().await;
    let id = d.people["olga"].user_id;
    req(&mut d, "mark", "POST", &format!("/api/admin/users/{id}/deactivate"), json!({})).await;
    req(&mut d, "mark", "POST", &format!("/api/admin/users/{id}/reactivate"), json!({})).await;
    let password=req(&mut d,"mark","POST",&format!("/api/admin/users/{id}/reset-password"),json!({})).await["one_time_password"].as_str().unwrap().to_owned();
    req(&mut d, "mark", "POST", &format!("/api/admin/users/{id}/reset-totp"), json!({})).await;
    d.expect("olga", "GET", "/api/staff/cases", json!({}), 401).await.unwrap();
    req(&mut d, "recovered", "GET", "/api/me", json!({})).await;
    let login = req(
        &mut d,
        "recovered",
        "POST",
        "/api/auth/login",
        json!({"email":"olga@demo.servicehub.invalid","password":password}),
    )
    .await;
    assert_eq!(login["mfa_required"], true);
    let enrollment = req(&mut d, "recovered", "POST", "/api/auth/totp/enroll", json!({})).await;
    let secret = enrollment["secret"].as_str().unwrap();
    let code = servicehub::auth::totp::code_at(secret, d.state.now().timestamp() as u64).unwrap();
    let me = req(&mut d, "recovered", "POST", "/api/auth/totp/enroll/confirm", json!({"code":code})).await;
    assert_eq!(me["password_change_required"], true);
    d.expect("recovered", "GET", "/api/staff/cases", json!({}), 403).await.unwrap();
    let new = "A new personal password 123";
    req(
        &mut d,
        "recovered",
        "POST",
        "/api/auth/change-password",
        json!({"current_password":password,"new_password":new}),
    )
    .await;
    req(&mut d, "recovered", "GET", "/api/staff/cases", json!({})).await;
    d.expect("recovered", "POST", "/api/auth/logout", json!({}), 204).await.unwrap();
    req(&mut d, "recovered", "GET", "/api/me", json!({})).await;
    d.expect(
        "recovered",
        "POST",
        "/api/auth/login",
        json!({"email":"olga@demo.servicehub.invalid","password":password}),
        401,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn configured_finance_and_works_copies_are_used_and_complaints_stay_private() {
    let (mut d, _dir) = support::fixture().await;
    req(&mut d,"mark","POST","/api/admin/settings",json!({"notify.finance_email":"finance-copy@example.invalid","notify.works_depot_email":"works-copy@example.invalid","notify.customer_care_email":"care-copy@example.invalid"})).await;
    let c = paid_hall(&mut d, "alexey", 17).await;
    assert!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM notifications WHERE case_id=? AND to_address='works-copy@example.invalid'"
        )
        .bind(c)
        .fetch_one(&d.state.db)
        .await
        .unwrap()
            > 0
    );
    let b = req(&mut d, "olga", "GET", &format!("/api/cases/{c}/booking"), json!({})).await;
    req(
        &mut d,
        "olga",
        "POST",
        &format!("/api/cases/{c}/booking/cancel"),
        json!({"reason":"Unused; finance review","expected_revision":b["booking"]["revision"]}),
    )
    .await;
    assert!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM notifications WHERE case_id=? AND to_address='finance-copy@example.invalid'"
        )
        .bind(c)
        .fetch_one(&d.state.db)
        .await
        .unwrap()
            > 0
    );
    let (c, _) = d.submit("alexey", "complaint", json!({}), None).await.unwrap();
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM notifications WHERE case_id=? AND to_address IN ('finance-copy@example.invalid','works-copy@example.invalid','care-copy@example.invalid')").bind(c).fetch_one(&d.state.db).await.unwrap(),0);
}

#[test]
fn docker_web_dist_cannot_be_overridden_by_native_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let example = std::fs::read_to_string(root.join(".env.example")).unwrap();
    assert!(!example.lines().map(str::trim).any(|line| line.starts_with("WEB_DIST=")));
    let compose = std::fs::read_to_string(root.join("docker-compose.yml")).unwrap();
    assert!(compose.lines().map(str::trim).any(|line| line == "WEB_DIST: /app/web"));
    let image = std::fs::read_to_string(root.join("Dockerfile")).unwrap();
    assert!(image.contains("/app/web"));
}
