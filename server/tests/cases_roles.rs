//! Case actions belong to one audience: the applicant's own draft is theirs alone, finance only moves its money
//! steps, and staff act for an applicant only on assisted requests, labelled as recorded on their behalf.
mod support;
use serde_json::{Value, json};

fn actions(detail: &Value) -> Vec<&str> {
    detail["allowed_actions"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect()
}

#[tokio::test]
async fn staff_cannot_read_change_submit_or_delete_an_applicants_draft() {
    let (mut d, _dir) = support::fixture().await;
    let c = d.req("alexey", "POST", "/api/services/planning-certificate/drafts", json!({})).await.unwrap()["id"]
        .as_i64()
        .unwrap();
    let draft = format!("/api/cases/{c}/draft");
    d.req("alexey", "PUT", &draft, json!({"answers":{"sections":"Zoning"}})).await.unwrap();
    for staff in ["olga", "priya", "helen", "tom"] {
        d.expect(staff, "GET", &draft, json!({}), 404).await.unwrap();
        d.expect(staff, "PUT", &draft, json!({"answers":{"sections":"Changed by staff"}}), 404).await.unwrap();
        d.expect(staff, "POST", &format!("/api/cases/{c}/submit"), json!({}), 404).await.unwrap();
        d.expect(staff, "DELETE", &draft, json!({}), 404).await.unwrap();
        d.expect(staff, "GET", &format!("/api/cases/{c}"), json!({}), 404).await.unwrap();
        let listed = d.req(staff, "GET", "/api/staff/cases?queue=all&page_size=100", json!({})).await.unwrap();
        assert!(!listed["items"].as_array().unwrap().iter().any(|i| i["id"] == c), "{staff} lists the draft");
    }
    let loaded = d.req("alexey", "GET", &draft, json!({})).await.unwrap();
    assert_eq!(loaded["answers"]["sections"], "Zoning");
    d.req("alexey", "DELETE", &draft, json!({})).await.unwrap();
    // A deleted, never-submitted draft (its uploads are kept) stays the applicant's: no staff route reaches it.
    for staff in ["olga", "helen", "tom"] {
        let listed = d.req(staff, "GET", "/api/staff/cases?queue=all&page_size=100", json!({})).await.unwrap();
        assert!(!listed["items"].as_array().unwrap().iter().any(|i| i["id"] == c), "a deleted draft is not a request");
        d.expect(staff, "GET", &format!("/api/cases/{c}"), json!({}), 404).await.unwrap();
        d.expect(staff, "GET", &format!("/api/cases/{c}/documents"), json!({}), 404).await.unwrap();
    }
}

#[tokio::test]
async fn an_assisted_draft_is_continued_only_by_the_staff_member_who_recorded_it() {
    let (mut d, _dir) = support::fixture().await;
    let (_, answers) = d.answers("olga", "road-issue").await.unwrap();
    let c = d
        .req("olga", "POST", "/api/staff/intake", json!({"service":"road-issue","channel":"phone","applicant_name":"Fictional caller","applicant_phone":"+672355501","answers":answers,"draft_only":true}))
        .await
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    for staff in ["priya", "helen"] {
        d.expect(staff, "PUT", &format!("/api/cases/{c}/draft"), json!({"answers":answers}), 403).await.unwrap();
        d.expect(staff, "POST", &format!("/api/cases/{c}/submit"), json!({}), 403).await.unwrap();
    }
    d.req("olga", "GET", &format!("/api/cases/{c}/draft"), json!({})).await.unwrap();
}

#[tokio::test]
async fn finance_cannot_refuse_or_ask_the_applicant_for_information() {
    let (mut d, _dir) = support::fixture().await;
    let slot = d.hall("rawson-main", 21);
    let (c, _) = d.submit("alexey", "rawson-hall-hire", slot, None).await.unwrap();
    d.action("olga", c, "advance").await.unwrap();
    let tom = d.detail("tom", c).await.unwrap();
    let allowed = actions(&tom);
    assert!(allowed.contains(&"advance"), "finance moves its payment step: {allowed:?}");
    assert!(!allowed.contains(&"refuse") && !allowed.contains(&"request-info"), "{allowed:?}");
    let revision = d.revision("tom", c).await.unwrap();
    for action in ["refuse", "request-info"] {
        d.expect(
            "tom",
            "POST",
            &format!("/api/cases/{c}/actions/{action}"),
            json!({"expected_revision":revision,"reason":"Finance should not do this."}),
            403,
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn staff_record_the_applicants_reply_only_on_an_assisted_request() {
    let (mut d, _dir) = support::fixture().await;
    let (_, answers) = d.answers("olga", "road-issue").await.unwrap();
    let assisted = d
        .req("olga", "POST", "/api/staff/intake", json!({"service":"road-issue","channel":"phone","applicant_name":"Fictional caller","applicant_phone":"+672355501","answers":answers}))
        .await
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let (online, _) = d.submit("alexey", "road-issue", json!({}), None).await.unwrap();
    for c in [assisted, online] {
        let revision = d.revision("olga", c).await.unwrap();
        d.req(
            "olga",
            "POST",
            &format!("/api/cases/{c}/actions/request-info"),
            json!({"expected_revision":revision,"body":"Which side of the road is the pothole on?"}),
        )
        .await
        .unwrap();
    }

    // Online: the applicant answers online; staff cannot answer for them.
    assert!(!actions(&d.detail("olga", online).await.unwrap()).contains(&"record-reply"));
    let revision = d.revision("olga", online).await.unwrap();
    d.expect(
        "olga",
        "POST",
        &format!("/api/cases/{online}/actions/record-reply"),
        json!({"expected_revision":revision,"channel":"phone","body":"North side."}),
        403,
    )
    .await
    .unwrap();

    // Assisted: a labelled staff action, with the channel required; finance cannot use it.
    assert!(actions(&d.detail("olga", assisted).await.unwrap()).contains(&"record-reply"));
    assert!(!actions(&d.detail("tom", assisted).await.unwrap()).contains(&"record-reply"));
    let path = format!("/api/cases/{assisted}/actions/record-reply");
    let revision = d.revision("olga", assisted).await.unwrap();
    let missing =
        d.expect("olga", "POST", &path, json!({"expected_revision":revision,"body":"North side."}), 422).await.unwrap();
    assert!(missing["error"]["fields"].get("channel").is_some(), "{missing}");
    d.req(
        "olga",
        "POST",
        &path,
        json!({"expected_revision":revision,"channel":"phone","body":"North side, near the bus stop."}),
    )
    .await
    .unwrap();
    let detail = d.detail("olga", assisted).await.unwrap();
    assert_eq!(detail["case"]["status"], "in_progress");
    assert!(detail["required_action"].is_null());
    let recorded = detail["timeline"].as_array().unwrap().last().unwrap()["summary"].as_str().unwrap().to_owned();
    assert!(recorded.contains("by phone") && recorded.contains("on the applicant's behalf"), "{recorded}");
    let thread = d.req("olga", "GET", &format!("/api/cases/{assisted}/messages"), json!({})).await.unwrap();
    let reply = thread["items"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(reply["from_staff"], false);
    assert!(reply["body"].as_str().unwrap().contains("recorded by Council on the applicant's behalf"), "{reply}");
}
