//! Audit-2 slice A (N-01, N-02, N-03) through the HTTP API: building fees and payment gate, DA/BA scope with
//! per-chain modifications, and a public exhibition that cannot be bypassed.
mod support;
use chrono::Duration;
use serde_json::{Value, json};
use servicehub::seed::{driver::Driver, scenarios};

const BOTH: [&str; 2] = ["development_approval", "building_approval"];

async fn submit_da(d: &mut Driver, approvals: &[&str], cost: i64) -> i64 {
    let (c, _) = d
        .submit("alexey", "development-application", json!({"approvals_sought":approvals,"estimated_cost":cost}), None)
        .await
        .unwrap();
    d.action("olga", c, "advance").await.unwrap();
    c
}
async fn submit_modification(d: &mut Driver, originals: &[i64], types: &[&str]) -> i64 {
    let answers = json!({"original_approval":{"decision_ids":originals},"modification_types":types,"modification_type":if types == ["lapse_date"] {"Lapse date"} else {"Conditions"},"proposed_lapse_date":"2027-12-01"});
    let (c, _) = d.submit("alexey", "modify-approval", answers, None).await.unwrap();
    d.action("olga", c, "advance").await.unwrap();
    c
}
/// Fee assessed and paid, scope confirmed, exhibition recorded as not required; the case waits for decisions.
async fn to_decision(d: &mut Driver, c: i64, scope: Value) {
    scenarios::assess_fee(d, c).await.unwrap();
    d.pay("alexey", c, false).await.unwrap();
    assert_eq!(d.detail("olga", c).await.unwrap()["case"]["current_step"], "assessment");
    scenarios::confirm_scope(d, c, scope).await.unwrap();
    d.action("priya", c, "advance").await.unwrap();
    scenarios::exhibition_not_required(d, c, "Fictional demo: notified by letter; no public comment period applies.")
        .await
        .unwrap();
    d.action("priya", c, "advance").await.unwrap();
    assert_eq!(d.detail("olga", c).await.unwrap()["case"]["current_step"], "decision");
}
async fn status(d: &mut Driver, c: i64) -> String {
    d.detail("alexey", c).await.unwrap()["case"]["status"].as_str().unwrap().to_string()
}
async fn revision(d: &mut Driver, who: &str, c: i64) -> i64 {
    d.revision(who, c).await.unwrap()
}
async fn prepare(d: &mut Driver, c: i64, kind: &str, supersedes: Option<i64>) -> i64 {
    let templates = d.req("priya", "GET", "/api/decision-templates", json!({})).await.unwrap();
    let template = templates.as_array().unwrap().iter().find(|t| t["decision_type"] == kind).unwrap()["id"].clone();
    let r = revision(d, "priya", c).await;
    let id = d.req("priya","POST",&format!("/api/cases/{c}/decisions"),json!({"decision_type":kind,"outcome":"approved","reasons":"Fictional assessment.","conditions":"","template_id":template,"supersedes_decision_id":supersedes,"expected_revision":r})).await.unwrap()["id"].as_i64().unwrap();
    let r = revision(d, "priya", c).await;
    d.req("priya", "POST", &format!("/api/cases/{c}/decisions/{id}/submit"), json!({"expected_revision":r}))
        .await
        .unwrap();
    id
}
async fn issue(d: &mut Driver, c: i64, id: i64, expected: u16) -> Value {
    let r = revision(d, "helen", c).await;
    d.expect("helen", "POST", &format!("/api/cases/{c}/decisions/{id}/issue"), json!({"expected_revision":r}), expected)
        .await
        .unwrap()
}
async fn assess(d: &mut Driver, c: i64, body: Value) -> Value {
    let mut body = body;
    body["expected_revision"] = json!(revision(d, "olga", c).await);
    d.req("olga", "POST", &format!("/api/cases/{c}/building-fee"), body).await.unwrap()
}
fn invoices(money: &Value, kind: &str) -> Vec<Value> {
    let mut list: Vec<Value> =
        money["invoices"].as_array().unwrap().iter().filter(|i| i["kind"] == kind).cloned().collect();
    list.sort_by_key(|i| i["id"].as_i64());
    list
}

#[tokio::test]
async fn primary_application_fee_is_assessed_invoiced_paid_and_reassessed_without_touching_the_old_invoice() {
    let (mut d, dir) = support::fixture().await;
    assert!(dir.path().is_dir());
    let c = submit_da(&mut d, &BOTH, 120_000).await;
    // No fee assessment yet: the fee step cannot be completed and nothing is invoiced.
    let r = revision(&mut d, "olga", c).await;
    d.expect("olga", "POST", &format!("/api/cases/{c}/actions/advance"), json!({"expected_revision":r}), 409)
        .await
        .unwrap();
    let fee = d.req("olga", "GET", &format!("/api/cases/{c}/building-fee"), json!({})).await.unwrap();
    assert_eq!(fee["proposal"]["amount_cents"], 88_000);
    let explanation = fee["proposal"]["explanation"].as_str().unwrap();
    assert!(explanation.contains("$120000.00") && explanation.contains("$600.00 + $4.00 per $1,000"), "{explanation}");
    assert!(explanation.contains("demo copy"));
    scenarios::assess_fee(&mut d, c).await.unwrap();
    let money = d.money("alexey", c).await.unwrap();
    let first = invoices(&money, "invoice");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["total_cents"], 88_000);
    // Unpaid: the payment step holds.
    let r = revision(&mut d, "tom", c).await;
    d.expect("tom", "POST", &format!("/api/cases/{c}/actions/advance"), json!({"expected_revision":r}), 409)
        .await
        .unwrap();
    d.pay("alexey", c, false).await.unwrap();
    scenarios::confirm_scope(&mut d, c, json!({"approvals":BOTH})).await.unwrap();
    d.action("priya", c, "advance").await.unwrap();
    scenarios::exhibition_not_required(&mut d, c, "Fictional demo: notified by letter.").await.unwrap();
    d.action("priya", c, "advance").await.unwrap();
    // The estimated cost changes: a new, explained assessment and a supplementary invoice; the old one is unchanged.
    let reassessed = assess(
        &mut d,
        c,
        json!({"method":"schedule","estimated_cost":"300000","reason":"Applicant revised the estimated cost to $300,000."}),
    )
    .await;
    assert_eq!(reassessed["version"], 2);
    assert_eq!(reassessed["amount_cents"], 172_850);
    let money = d.money("alexey", c).await.unwrap();
    let all = invoices(&money, "invoice");
    assert_eq!(all.len(), 2);
    let original = all.iter().find(|i| i["id"] == first[0]["id"]).unwrap();
    assert_eq!(original["number"], first[0]["number"]);
    assert_eq!(original["total_cents"], 88_000);
    assert_eq!(original["outstanding_cents"], 0);
    let supplementary = all.iter().find(|i| i["id"] != first[0]["id"]).unwrap();
    assert_eq!(supplementary["total_cents"], 84_850);
    let fee = d.req("alexey", "GET", &format!("/api/cases/{c}/building-fee"), json!({})).await.unwrap();
    let history = fee["assessments"].as_array().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0]["inputs"]["estimated_cost_source"], "staff");
    assert_eq!(history[0]["reason"], "Applicant revised the estimated cost to $300,000.");
    assert!(history[0]["explanation"].as_str().unwrap().contains("$1600.00 + $2.57 per $1,000"));
    assert_eq!(history[0]["adjustment"]["total_cents"], 84_850);
    assert!(history[0]["assessed_by"].as_str().is_some());
    // Decisions cannot be issued while the difference is unpaid.
    let da = prepare(&mut d, c, "development_approval", None).await;
    let error = issue(&mut d, c, da, 409).await;
    assert!(error["error"]["message"].as_str().unwrap().contains("not fully paid"), "{error}");
    d.pay("alexey", c, false).await.unwrap();
    issue(&mut d, c, da, 200).await;
    d.decision(c, "building_approval", None).await.unwrap();
    assert_eq!(status(&mut d, c).await, "completed");
}

#[tokio::test]
async fn modification_fee_rules_staff_assessment_credit_note_and_explicit_waiver() {
    let (mut d, _dir) = support::fixture().await;
    let project = submit_da(&mut d, &["development_approval"], 50_000).await;
    to_decision(&mut d, project, json!({"approvals":["development_approval"]})).await;
    let da = d.decision(project, "development_approval", None).await.unwrap();
    // Basic (lapse date only) modification: the $250 price item.
    let basic = submit_modification(&mut d, &[da], &["lapse_date"]).await;
    let fee = d.req("olga", "GET", &format!("/api/cases/{basic}/building-fee"), json!({})).await.unwrap();
    assert_eq!(fee["proposal"]["rule"], "basic_modification");
    assert_eq!(fee["proposal"]["amount_cents"], 25_000);
    scenarios::assess_fee(&mut d, basic).await.unwrap();
    let money = d.money("alexey", basic).await.unwrap();
    assert_eq!(invoices(&money, "invoice")[0]["total_cents"], 25_000);
    // Staff decide the modification also changes conditions: standard modification on the scale.
    let standard =
        assess(&mut d, basic, json!({"method":"schedule","modification_types":["lapse_date","conditions"],"reason":"Conditions also change."}))
            .await;
    assert_eq!(standard["amount_cents"], 57_000);
    let money = d.money("alexey", basic).await.unwrap();
    assert_eq!(invoices(&money, "invoice").len(), 2);
    assert_eq!(invoices(&money, "invoice")[1]["total_cents"], 32_000);
    // Re-assessed back down: a credit note returns the difference; no issued invoice is edited.
    assess(&mut d, basic, json!({"method":"schedule","modification_types":["lapse_date"],"reason":"Conditions stay unchanged after all."}))
        .await;
    let money = d.money("alexey", basic).await.unwrap();
    let credit = invoices(&money, "credit_note");
    assert_eq!(credit.len(), 1);
    assert_eq!(credit[0]["total_cents"], 32_000);
    assert_eq!(invoices(&money, "invoice")[0]["total_cents"], 25_000);
    assert_eq!(money["summary"]["outstanding_cents"], 25_000);
    d.pay("alexey", basic, false).await.unwrap();
    assert_eq!(d.detail("olga", basic).await.unwrap()["case"]["current_step"], "assessment");

    // Staff assessment with basis, then an explicit manager waiver before invoicing: a zero invoice on record.
    let waived = submit_modification(&mut d, &[da], &["other"]).await;
    let r = revision(&mut d, "olga", waived).await;
    let error = d
        .expect(
            "olga",
            "POST",
            &format!("/api/cases/{waived}/building-fee"),
            json!({"method":"manual","amount_cents":40_000,"expected_revision":r}),
            422,
        )
        .await
        .unwrap();
    assert!(error["error"]["fields"]["reason"].is_string());
    assess(&mut d, waived, json!({"method":"manual","amount_cents":40_000,"reason":"Fictional: assessed by Council officer on the revised scope."})).await;
    let fee = d.req("alexey", "GET", &format!("/api/cases/{waived}/building-fee"), json!({})).await.unwrap();
    assert_eq!(fee["assessments"][0]["method"], "manual");
    assert!(fee["assessments"][0]["explanation"].as_str().unwrap().contains("Schedule calculation for reference"));
    let r = revision(&mut d, "helen", waived).await;
    d.expect("helen","POST",&format!("/api/cases/{waived}/price-waivers"),json!({"item_code":"BUILDING_WORKS_FEE","amount_cents":40_000,"reason":"Fictional community group exemption","expected_revision":r}),201).await.unwrap();
    d.action("olga", waived, "advance").await.unwrap();
    let money = d.money("alexey", waived).await.unwrap();
    let invoice = &invoices(&money, "invoice")[0];
    assert_eq!(invoice["total_cents"], 0);
    assert!(invoice["lines"].as_array().unwrap().iter().any(|l| l["description"].as_str().unwrap().contains("Waiver")));
    d.action("tom", waived, "advance").await.unwrap();
    assert_eq!(d.detail("olga", waived).await.unwrap()["case"]["current_step"], "assessment");
}

#[tokio::test]
async fn da_only_ba_only_and_combined_cases_need_only_their_decisions_and_modifications_follow_each_chain() {
    let (mut d, _dir) = support::fixture().await;
    // DA only (the applicant asked for both; staff narrow the scope with a reason).
    let da_only = submit_da(&mut d, &BOTH, 80_000).await;
    to_decision(&mut d, da_only, json!({"approvals":["development_approval"]})).await;
    let templates = d.req("priya", "GET", "/api/decision-templates", json!({})).await.unwrap();
    let template =
        templates.as_array().unwrap().iter().find(|t| t["decision_type"] == "building_approval").unwrap()["id"].clone();
    let r = revision(&mut d, "priya", da_only).await;
    d.expect("priya","POST",&format!("/api/cases/{da_only}/decisions"),json!({"decision_type":"building_approval","outcome":"approved","reasons":"x","template_id":template,"expected_revision":r}),422).await.unwrap();
    d.decision(da_only, "development_approval", None).await.unwrap();
    assert_eq!(status(&mut d, da_only).await, "completed");
    let route = d.req("alexey", "GET", &format!("/api/cases/{da_only}/building-route"), json!({})).await.unwrap();
    assert_eq!(route["scope"]["approvals"], json!(["development_approval"]));
    assert_eq!(route["scope_history"].as_array().unwrap().len(), 2);
    assert!(
        d.detail("alexey", da_only).await.unwrap()["timeline"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["summary"].as_str().unwrap().contains("Approval scope confirmed: Development approval."))
    );
    // BA only.
    let ba_only = submit_da(&mut d, &["building_approval"], 80_000).await;
    to_decision(&mut d, ba_only, json!({"approvals":["building_approval"]})).await;
    d.decision(ba_only, "building_approval", None).await.unwrap();
    assert_eq!(status(&mut d, ba_only).await, "completed");
    // Combined.
    let combined = submit_da(&mut d, &BOTH, 80_000).await;
    to_decision(&mut d, combined, json!({"approvals":BOTH})).await;
    let da = d.decision(combined, "development_approval", None).await.unwrap();
    assert_ne!(status(&mut d, combined).await, "completed");
    let ba = d.decision(combined, "building_approval", None).await.unwrap();
    assert_eq!(status(&mut d, combined).await, "completed");
    let project = d.req("alexey", "GET", &format!("/api/cases/{combined}/decisions"), json!({})).await.unwrap()
        ["building_project_id"]
        .as_i64()
        .unwrap();

    // Modify only the DA of the combined case: the BA chain stays current and untouched.
    let modify_da = submit_modification(&mut d, &[da], &["conditions"]).await;
    to_decision(&mut d, modify_da, json!({"originals":[da]})).await;
    let mod_da = d.decision_for(modify_da, "modification_approval", None, Some(da)).await.unwrap();
    assert_eq!(status(&mut d, modify_da).await, "completed");
    let p = d.req("alexey", "GET", &format!("/api/building-projects/{project}"), json!({})).await.unwrap();
    let chain = |p: &Value, root: i64| {
        p["chains"].as_array().unwrap().iter().find(|c| c["root_decision_id"] == root).unwrap().clone()
    };
    assert_eq!(chain(&p, da)["current_decision_id"], mod_da);
    assert_eq!(chain(&p, da)["versions"][0]["state"], "superseded");
    assert_eq!(chain(&p, ba)["current_decision_id"], ba);
    assert_eq!(chain(&p, ba)["versions"].as_array().unwrap().len(), 1);

    // A superseded approval can no longer be modified; the joint DA+BA modification names both current versions.
    let rejected = d
        .submit(
            "alexey",
            "modify-approval",
            json!({"original_approval":{"decision_ids":[da]},"modification_types":["conditions"]}),
            None,
        )
        .await
        .unwrap_err();
    assert!(rejected.message.contains("still current"), "{}", rejected.message);
    let joint = submit_modification(&mut d, &[mod_da, ba], &["conditions"]).await;
    scenarios::assess_fee(&mut d, joint).await.unwrap();
    d.pay("alexey", joint, false).await.unwrap();
    // Staff first narrow to the BA, then restore both with a reason; each change is recorded.
    scenarios::confirm_scope(&mut d, joint, json!({"originals":[ba]})).await.unwrap();
    scenarios::confirm_scope(&mut d, joint, json!({"originals":[mod_da, ba]})).await.unwrap();
    let route = d.req("olga", "GET", &format!("/api/cases/{joint}/building-route"), json!({})).await.unwrap();
    assert_eq!(route["scope_history"].as_array().unwrap().len(), 3);
    assert_eq!(route["originals"].as_array().unwrap().len(), 2);
    d.action("priya", joint, "advance").await.unwrap();
    scenarios::exhibition_not_required(&mut d, joint, "Fictional demo: minor change, no exhibition.").await.unwrap();
    d.action("priya", joint, "advance").await.unwrap();
    // A modification decision must name which original it supersedes.
    let templates = d.req("priya", "GET", "/api/decision-templates", json!({})).await.unwrap();
    let template =
        templates.as_array().unwrap().iter().find(|t| t["decision_type"] == "modification_approval").unwrap()["id"]
            .clone();
    let r = revision(&mut d, "priya", joint).await;
    d.expect("priya","POST",&format!("/api/cases/{joint}/decisions"),json!({"decision_type":"modification_approval","outcome":"approved","reasons":"x","template_id":template,"expected_revision":r}),422).await.unwrap();
    let joint_da = d.decision_for(joint, "modification_approval", None, Some(mod_da)).await.unwrap();
    assert_ne!(status(&mut d, joint).await, "completed");
    let joint_ba = d.decision_for(joint, "modification_approval", None, Some(ba)).await.unwrap();
    assert_eq!(status(&mut d, joint).await, "completed");
    let p = d.req("alexey", "GET", &format!("/api/building-projects/{project}"), json!({})).await.unwrap();
    let da_chain = chain(&p, da);
    assert_eq!(da_chain["current_decision_id"], joint_da);
    assert_eq!(
        da_chain["versions"].as_array().unwrap().iter().map(|v| v["id"].as_i64().unwrap()).collect::<Vec<_>>(),
        vec![da, mod_da, joint_da]
    );
    let ba_chain = chain(&p, ba);
    assert_eq!(ba_chain["current_decision_id"], joint_ba);
    assert_eq!(ba_chain["versions"][1]["supersedes_decision_id"], ba);
    // Both originals of the joint modification stay linked to their own new version.
    let decisions = d.req("alexey", "GET", &format!("/api/cases/{joint}/decisions"), json!({})).await.unwrap();
    let mut supersedes: Vec<i64> =
        decisions["items"].as_array().unwrap().iter().map(|d| d["supersedes_decision_id"].as_i64().unwrap()).collect();
    supersedes.sort_unstable();
    let mut expected = vec![mod_da, ba];
    expected.sort_unstable();
    assert_eq!(supersedes, expected);
}

#[tokio::test]
async fn public_exhibition_blocks_skip_and_decisions_until_closed_and_considered() {
    let (mut d, _dir) = support::fixture().await;
    let now = d.state.now();
    // Case A: an open exhibition with a comment.
    let a = submit_da(&mut d, &BOTH, 60_000).await;
    scenarios::assess_fee(&mut d, a).await.unwrap();
    d.pay("alexey", a, false).await.unwrap();
    scenarios::confirm_scope(&mut d, a, json!({"approvals":BOTH})).await.unwrap();
    d.action("priya", a, "advance").await.unwrap();
    let source = d.req("priya", "GET", &format!("/api/cases/{a}/documents"), json!({})).await.unwrap()[0]["versions"]
        [0]["id"]
        .as_i64()
        .unwrap();
    let (exhibit, _) =
        scenarios::publish_exhibition(&mut d, a, source, now - Duration::hours(1), now + Duration::days(14))
            .await
            .unwrap();
    scenarios::public_comment(&mut d, exhibit, "Fictional concern about stormwater.").await.unwrap();
    let detail = d.detail("priya", a).await.unwrap();
    assert!(!detail["allowed_actions"].as_array().unwrap().iter().any(|v| v == "skip"));
    assert!(detail["guard_reason"].as_str().unwrap().contains("open until"));
    let r = revision(&mut d, "priya", a).await;
    d.expect(
        "priya",
        "POST",
        &format!("/api/cases/{a}/actions/skip"),
        json!({"expected_revision":r,"reason":"Skip it"}),
        403,
    )
    .await
    .unwrap();
    d.expect("priya", "POST", &format!("/api/cases/{a}/actions/advance"), json!({"expected_revision":r}), 409)
        .await
        .unwrap();
    d.expect(
        "priya",
        "POST",
        &format!("/api/cases/{a}/exhibition-not-required"),
        json!({"reason":"Not needed","expected_revision":r}),
        409,
    )
    .await
    .unwrap();
    let held = prepare(&mut d, a, "development_approval", None).await;
    issue(&mut d, a, held, 409).await;
    // Window closed: the comment still needs a recorded outcome.
    d.seed_time(now + Duration::days(15)).await.unwrap();
    let r = revision(&mut d, "priya", a).await;
    let blocked = d
        .expect("priya", "POST", &format!("/api/cases/{a}/actions/advance"), json!({"expected_revision":r}), 409)
        .await
        .unwrap();
    assert!(blocked["error"]["message"].as_str().unwrap().contains("consideration"));
    d.req(
        "priya",
        "POST",
        &format!("/api/exhibitions/{exhibit}/consideration"),
        json!({"reason":"Fictional: stormwater addressed by condition 4.","expected_revision":r}),
    )
    .await
    .unwrap();
    let rows = d.req("priya", "GET", &format!("/api/exhibitions/{exhibit}/submissions"), json!({})).await.unwrap();
    assert!(rows[0]["outcome"].as_str().unwrap().contains("stormwater addressed"));
    d.action("priya", a, "advance").await.unwrap();
    issue(&mut d, a, held, 200).await;
    d.decision(a, "building_approval", None).await.unwrap();
    assert_eq!(status(&mut d, a).await, "completed");

    // Case B: exhibition reopened at the decision step blocks issue; formal termination, then consideration.
    let b = submit_da(&mut d, &["development_approval"], 60_000).await;
    to_decision(&mut d, b, json!({"approvals":["development_approval"]})).await;
    let source = d.req("priya", "GET", &format!("/api/cases/{b}/documents"), json!({})).await.unwrap()[0]["versions"]
        [0]["id"]
        .as_i64()
        .unwrap();
    let now = d.state.now();
    let (late, _) =
        scenarios::publish_exhibition(&mut d, b, source, now - Duration::hours(1), now + Duration::days(14))
            .await
            .unwrap();
    let comment = scenarios::public_comment(&mut d, late, "Fictional late objection.").await.unwrap();
    let decision = prepare(&mut d, b, "development_approval", None).await;
    let error = issue(&mut d, b, decision, 409).await;
    assert!(error["error"]["message"].as_str().unwrap().contains("exhibition"), "{error}");
    let r = revision(&mut d, "priya", b).await;
    d.req(
        "priya",
        "POST",
        &format!("/api/exhibitions/{late}/terminate"),
        json!({"reason":"Fictional: notice published with the wrong lot number.","expected_revision":r}),
    )
    .await
    .unwrap();
    let public = d.req("stranger", "GET", &format!("/api/public/exhibitions/{late}"), json!({})).await.unwrap();
    assert_eq!(public["exhibition"]["status"], "terminated");
    assert!(public["exhibition"]["termination_reason"].as_str().unwrap().contains("wrong lot number"));
    let error = issue(&mut d, b, decision, 409).await;
    assert!(error["error"]["message"].as_str().unwrap().contains("consideration"), "{error}");
    let r = revision(&mut d, "priya", b).await;
    d.req(
        "priya",
        "POST",
        &format!("/api/exhibitions/{late}/submissions/{comment}/consider"),
        json!({"outcome":"Fictional: objection noted; lot number corrected.","expected_revision":r}),
    )
    .await
    .unwrap();
    issue(&mut d, b, decision, 200).await;
    assert_eq!(status(&mut d, b).await, "completed");

    // Case C: a justified "not required" is recorded and visible to the applicant.
    let c = submit_da(&mut d, &["building_approval"], 60_000).await;
    to_decision(&mut d, c, json!({"approvals":["building_approval"]})).await;
    let route = d.req("alexey", "GET", &format!("/api/cases/{c}/building-route"), json!({})).await.unwrap();
    assert!(route["exhibition"]["not_required"]["reason"].as_str().unwrap().contains("notified by letter"));
    assert!(
        d.detail("alexey", c).await.unwrap()["timeline"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["summary"].as_str().unwrap().contains("Public exhibition is not required"))
    );
}
