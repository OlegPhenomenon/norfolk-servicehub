//! Fictional history driven through the same authenticated router and domain commands as visitors.
use chrono::Duration;
use serde_json::{Value, json};

use super::driver::Driver;
use crate::{AppResult, AppState, error::AppError, time};

pub async fn run(state: &AppState) -> AppResult<()> {
    // Replaying seed_demo on an already populated database does not replay business commands.
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM submissions").fetch_one(&state.db).await? > 0 {
        return Ok(());
    }
    let present = state.now();
    let mut d = Driver::new(state.clone(), present - Duration::days(28)).await?;
    d.seed_sessions().await?;
    // Three paid, confirmed, inspected and settled hires in the recent past.
    for (index, days) in [24, 17, 10].into_iter().enumerate() {
        d.seed_time(present - Duration::days(days + 3)).await?;
        let hall = free_hall(&mut d, "rawson-main", 3).await?;
        let (case, _) = d.submit("ben", "rawson-hall-hire", hall, None).await?;
        confirm_hall(&mut d, case).await?;
        d.complete_task(case, "venue_prep").await?;
        d.seed_time(present - Duration::days(days - 4)).await?;
        d.complete_task(case, "venue_inspection").await?;
        settle_bond(&mut d, case, if index == 0 { 5000 } else { 0 }).await?;
    }
    d.seed_time(present - Duration::days(9)).await?;
    let hall = free_hall(&mut d, "rawson-main", 3).await?;
    let (live_bond, _) = d.submit("alexey", "rawson-hall-hire", hall, None).await?;
    d.action("olga", live_bond, "advance").await?;
    d.pay("alexey", live_bond, false).await?;
    let booking = d.req("olga", "GET", &format!("/api/cases/{live_bond}/booking"), json!({})).await?["booking"].clone();
    d.req(
        "olga",
        "POST",
        &format!("/api/cases/{live_bond}/booking/confirm"),
        json!({"expected_revision":booking["revision"]}),
    )
    .await?;
    d.complete_task(live_bond, "venue_prep").await?;
    d.seed_time(present - Duration::days(2)).await?;
    d.complete_task(live_bond, "venue_inspection").await?;
    // Leave the paid bond undecided for the visitor.
    // Island Builders' revised plans and separate approvals, followed by linked requests.
    d.seed_time(present - Duration::days(20)).await?;
    let org: i64 = sqlx::query_scalar("SELECT organisation_id FROM memberships WHERE user_id=(SELECT id FROM users WHERE persona_key='ben') AND status='active'").fetch_one(&state.db).await?;
    let (building, docs) = d.submit("ben", "development-application", json!({}), Some(org)).await?;
    d.action("olga", building, "advance").await?;
    let drawing = &docs["floor_plans"];
    let revision = d.revision("priya", building).await?;
    let comment = d.req("priya", "POST", &format!("/api/document-versions/{}/comments", drawing["version_id"]), json!({"expected_revision":revision,"body":"Please replace drawing A-101 with corrected veranda dimensions.","visibility":"applicant","request_new_version":true})).await?["id"].clone();
    d.seed_time(present - Duration::days(18)).await?;
    let pdf = crate::pdf::simple_document(
        "Fictional drawing A-101 v2",
        &[],
        &[("Plan", "Revised veranda dimensions. Private phone: SECRET-PHONE-555-0199".into())],
    );
    let v2 = d
        .upload(
            "ben",
            &format!("/api/documents/{}/versions", drawing["id"]),
            &[("resolves_comment_ids", json!([comment]).to_string()), ("note", "Corrected drawing A-101".into())],
            &pdf,
        )
        .await?["version_id"]
        .as_i64()
        .unwrap();
    d.action("priya", building, "advance").await?;
    d.action("priya", building, "skip").await?;
    d.seed_time(present - Duration::days(15)).await?;
    d.decision(building, "development_approval", Some(vec![v2])).await?;
    let approval = d.decision(building, "building_approval", Some(vec![v2])).await?;
    let approvals = d.req("ben", "GET", "/api/my/issued-approvals", json!({})).await?;
    let project = approvals.as_array().unwrap().iter().find(|a| a["id"] == approval).unwrap()["project_id"].clone();
    d.seed_time(present - Duration::days(12)).await?;
    let (notice, _) = d
        .submit("ben", "building-commencement-notice", json!({"project_reference":project.to_string()}), Some(org))
        .await?;
    d.action("olga", notice, "advance").await?;
    d.action("olga", notice, "skip").await?;
    let (modification, _) =
        d.submit("ben", "modify-approval", json!({"original_approval":{"decision_id":approval}}), Some(org)).await?;
    d.action("olga", modification, "advance").await?;
    // Exhibition belongs to the still-open modification, rather than a completed approval.
    let public_plan = d
        .upload(
            "ben",
            &format!("/api/cases/{modification}/documents"),
            &[("title", "Revised modification plan A-101".into())],
            &pdf,
        )
        .await?["version_id"]
        .as_i64()
        .unwrap();
    publish_exhibition(&mut d, modification, public_plan, present - Duration::days(2), present + Duration::days(19))
        .await?;

    d.seed_time(present - Duration::days(8)).await?;
    let (cert, _) =
        d.submit("ben", "planning-certificate", json!({"sections":"Zoning and heritage"}), Some(org)).await?;
    d.action("olga", cert, "advance").await?;
    d.pay("ben", cert, false).await?;
    d.action("priya", cert, "advance").await?;
    d.decision(cert, "planning_certificate", None).await?;
    let (unpaid, _) = d
        .submit("ben", "planning-certificate", json!({"sections":"Property information for a proposed sale"}), None)
        .await?;
    d.action("olga", unpaid, "advance").await?;

    d.seed_time(present - Duration::days(7)).await?;
    let (equipment, _) = d.submit("ben", "equipment-hire", json!({"request":{"description":"Bobcat — fictional drainage preparation","requested_hours":4,"preferred_date":time::local_date(d.state.now()+Duration::days(2)).to_string(),"site_text":"Fictional builders depot, Taylors Road"}}), Some(org)).await?;
    d.action("olga", equipment, "advance").await?;
    let start = d.instant(2, 7, 30);
    let end = d.instant(2, 13, 0);
    schedule_equipment(&mut d, equipment, &start, &end).await?;
    d.seed_time(present - Duration::days(4)).await?;
    finish_equipment(&mut d, equipment, &start, &end).await?;
    d.pay("ben", equipment, false).await?;

    // Six geographically distinct reports: triage, inspection, repair, response, completed, duplicate.
    let mut roads = Vec::new();
    for index in 0..6 {
        d.seed_time(present - Duration::days(6 - index)).await?;
        let (road, _) = d.submit("ben", "road-issue", json!({"location":{"lat":-29.025-index as f64*0.008,"lng":167.925+index as f64*0.012,"description":format!("Fictional road report {}: pothole / blocked drain",index+1)}}), None).await?;
        roads.push(road);
        if index > 0 && index < 5 {
            d.action("olga", road, "advance").await?;
        }
        if index > 1 && index < 5 {
            d.complete_task(road, "road_inspection").await?;
        }
        if index > 2 && index < 5 {
            d.complete_task(road, "road_repair").await?;
        }
        if index == 4 {
            let revision = d.revision("olga", road).await?;
            d.req(
                "olga",
                "POST",
                &format!("/api/cases/{road}/road-response"),
                json!({"expected_revision":revision,"body":"The fictional pothole was repaired after inspection."}),
            )
            .await?;
        }
    }
    let duplicate = roads[5];
    let revision = d.revision("olga", duplicate).await?;
    d.req(
        "olga",
        "POST",
        &format!("/api/cases/{duplicate}/actions/close-duplicate"),
        json!({"expected_revision":revision,"reason":"Same road defect as the first report.","of_case":roads[0]}),
    )
    .await?;

    d.seed_time(present - Duration::days(9)).await?;
    let (complaint, _) = d.submit("alexey", "complaint", json!({}), None).await?;
    let olga = d.people["olga"].user_id;
    let revision = d.revision("ruth", complaint).await?;
    d.req(
        "ruth",
        "POST",
        &format!("/api/cases/{complaint}/complaint/subjects"),
        json!({"staff_user_ids":[olga],"expected_revision":revision}),
    )
    .await?;
    d.action("ruth", complaint, "advance").await?;
    d.action("ruth", complaint, "advance").await?;
    let revision = d.revision("ruth", complaint).await?;
    d.req("ruth","POST",&format!("/api/cases/{complaint}/letters"),json!({"letter_type":"complaint_response","title":"Fictional feedback response","body":"Ruth reviewed the feedback independently and provided a response.","expected_revision":revision})).await?;
    d.seed_time(present - Duration::days(3)).await?;
    d.req(
        "alexey",
        "POST",
        &format!("/api/cases/{complaint}/complaint/request-review"),
        json!({"reason":"Please independently review the response."}),
    )
    .await?;

    // Four assisted requests have contact details but no applicant user account.
    for index in 0..4 {
        d.seed_time(present - Duration::days(if index == 0 { 35 } else { 5 - index })).await?;
        let (_, mut answers) = d.answers("olga", "road-issue").await?;
        answers["applicant_name"] = json!(format!("Fictional caller {}", index + 1));
        let case = d.req("olga","POST","/api/staff/intake",json!({"service":"road-issue","channel":if index%2==0 {"phone"} else {"walk_in"},"applicant_name":format!("Fictional caller {}",index+1),"applicant_email":format!("caller{}@example.invalid",index+1),"applicant_phone":"+672355501","answers":answers})).await?["id"].as_i64().unwrap();
        if index == 1 {
            d.action("olga", case, "advance").await?;
        }
        if index == 2 {
            d.action("olga", case, "cancel").await?;
            d.action("helen", case, "reopen").await?;
        }
        if index == 3 {
            d.action("olga", case, "cancel").await?;
        }
    }
    d.seed_time(present).await?;
    for day in [7, 13] {
        let hall = free_hall(&mut d, if day == 7 { "rawson-main" } else { "rawson-supper" }, day).await?;
        let (case, _) = d.submit("ben", "rawson-hall-hire", hall, None).await?;
        confirm_hall(&mut d, case).await?;
    }
    let hall = free_hall(&mut d, "rawson-main", 19).await?;
    let (waiting, _) = d.submit("ben", "rawson-hall-hire", hall, None).await?;
    d.action("olga", waiting, "advance").await?;
    let (scheduled, _) = d.submit("ben","equipment-hire",json!({"request":{"description":"Bobcat — fictional garden access","requested_hours":4,"preferred_date":time::local_date(present+Duration::days(9)).to_string(),"site_text":"Fictional builders depot"}}),None).await?;
    d.action("olga", scheduled, "advance").await?;
    let start = d.instant(9, 7, 30);
    let end = d.instant(9, 13, 0);
    schedule_equipment(&mut d, scheduled, &start, &end).await?;
    // Appended after the earlier stories so their case numbers (used by the demo bank statement) stay put.
    // Builder's Stage B notice on Island Builders' project: returned for a signed copy, replaced and accepted.
    stage_notice(&mut d, &project.to_string(), org).await?;
    // Form 212 pipeline / conduit crossing through to a written decision.
    pipeline_crossing(&mut d).await?;

    let csv = include_str!("../../seed-data/legacy/legacy-cases-sample.csv");
    let batch = d
        .req("mark", "POST", "/api/admin/legacy-imports", json!({"filename":"legacy-cases-sample.csv","csv":csv}))
        .await?["id"]
        .as_i64()
        .unwrap();
    d.req(
        "mark",
        "POST",
        &format!("/api/admin/legacy-imports/{batch}/import"),
        json!({"skip_possible_duplicates":true}),
    )
    .await?;
    // A visible failed delivery followed by an audited manual retry and exactly one receiver row.
    integration_recovery(&mut d).await?;
    d.clock.advance(Duration::seconds(4));
    d.drain().await?;
    clock_sweep(&d).await?;
    verify(&mut d).await?;
    // Personas have already worked through historical updates. Keep at most three of today's
    // staff notifications unread for the visitor, without changing live notification delivery.
    sqlx::query("UPDATE notifications SET status='read',read_at=? WHERE channel='in_app' AND read_at IS NULL AND user_id IN (SELECT id FROM users WHERE kind='staff' AND persona_key IS NOT NULL) AND (created_at<? OR id NOT IN (SELECT recent.id FROM notifications recent WHERE recent.user_id=notifications.user_id AND recent.channel='in_app' AND recent.created_at>=? ORDER BY recent.id DESC LIMIT 3))")
        .bind(time::fmt(d.state.now())).bind(time::fmt(present)).bind(time::fmt(present))
        .execute(&state.db).await?;
    d.clear_sessions().await?;
    Ok(())
}

// Keep relative dates usable even when they coincide with a seeded maintenance window.
async fn free_hall(d: &mut Driver, unit: &str, day: i64) -> AppResult<Value> {
    for offset in 0..=2 {
        let from = d.instant(day + offset, 10, 0);
        let to = d.instant(day + offset, 16, 0);
        let availability = d
            .req("ben", "GET", &format!("/api/public/venues/rawson-hall/availability?from={from}&to={to}"), json!({}))
            .await?;
        if availability["units"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["code"] == unit && u["active"] == true && u["busy"].as_array().unwrap().is_empty())
        {
            return Ok(d.hall(unit, day + offset));
        }
    }
    Err(AppError::conflict("No free demonstration hall date in the seed window."))
}

pub async fn confirm_hall(d: &mut Driver, case: i64) -> AppResult<()> {
    d.action("olga", case, "advance").await?;
    d.pay("ben", case, false).await?;
    let booking = d.req("olga", "GET", &format!("/api/cases/{case}/booking"), json!({})).await?["booking"].clone();
    d.req(
        "olga",
        "POST",
        &format!("/api/cases/{case}/booking/confirm"),
        json!({"expected_revision":booking["revision"]}),
    )
    .await?;
    Ok(())
}
pub async fn settle_bond(d: &mut Driver, case: i64, retained: i64) -> AppResult<()> {
    let money = d.money("tom", case).await?;
    let bond = money["invoices"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "invoice")
        .flat_map(|i| i["lines"].as_array().unwrap())
        .find(|l| l["kind"] == "deposit")
        .unwrap();
    let revision = d.revision("tom", case).await?;
    let items = if retained == 0 { json!([]) } else { json!([{"label":"Extra cleaning","cents":retained}]) };
    d.req("tom","POST",&format!("/api/cases/{case}/deposit-decision"),json!({"invoice_line_id":bond["id"],"refund_cents":25000-retained,"retain_items":items,"reason":"Post-event inspection: cleaning and bond settlement.","expected_revision":revision})).await?;
    d.drain().await?; // provider refund request is dispatched, but completion is still scheduled.
    if d.detail("ben", case).await?["case"]["status"] == "completed" {
        return Err(AppError::internal("Bond closed before refund webhook"));
    }
    d.clock.advance(Duration::seconds(4));
    d.drain().await?;
    if d.detail("ben", case).await?["case"]["status"] != "completed" {
        return Err(AppError::internal("Refund webhook did not close hire"));
    }
    Ok(())
}
pub async fn schedule_equipment(d: &mut Driver, case: i64, start: &str, end: &str) -> AppResult<()> {
    let revision = d.revision("olga", case).await?;
    let jake = d.people["jake"].user_id;
    d.req("olga","POST",&format!("/api/cases/{case}/equipment/schedule"),json!({"resource_code":"EXCAVATOR","operator_user_id":jake,"start":start,"end":end,"expected_revision":revision})).await?;
    Ok(())
}
pub async fn finish_equipment(d: &mut Driver, case: i64, start: &str, end: &str) -> AppResult<Value> {
    let tasks = d.req("olga", "GET", &format!("/api/cases/{case}/tasks"), json!({})).await?;
    let task = tasks.as_array().unwrap().iter().find(|t| t["kind"] == "equipment_job").unwrap()["id"].as_i64().unwrap();
    let command = d.key();
    let usage=d.req("jake","POST",&format!("/api/field/tasks/{task}/usage"),json!({"client_command_id":command,"started_at":start,"ended_at":end,"downtime_minutes":30,"expenses_cents":1234,"expenses_note":"Agreed transport expenses"})).await?;
    d.complete_task(case, "equipment_job").await?;
    d.req("tom", "POST", &format!("/api/cases/{case}/equipment/usage/{}/approve", usage["usage_id"]), json!({}))
        .await?;
    Ok(usage)
}
pub async fn publish_exhibition(
    d: &mut Driver,
    case: i64,
    source: i64,
    opens: chrono::DateTime<chrono::Utc>,
    closes: chrono::DateTime<chrono::Utc>,
) -> AppResult<(i64, i64)> {
    let revision = d.revision("priya", case).await?;
    let exhibit=d.req("priya","POST","/api/exhibitions",json!({"case_id":case,"title":"Fictional veranda modification — public exhibition","summary":"Redacted demonstration plans; public comments invited.","opens_at":time::fmt(opens),"closes_at":time::fmt(closes),"expected_revision":revision})).await?["id"].as_i64().unwrap();
    let revision = d.revision("priya", case).await?;
    let item=d.req("priya","POST",&format!("/api/exhibitions/{exhibit}/items"),json!({"source_document_version_id":source,"title":"Redacted plan A-101","redactions":[{"page":1,"x":0.05,"y":0.1,"w":0.9,"h":0.35}],"expected_revision":revision})).await?["id"].as_i64().unwrap();
    let revision = d.revision("helen", case).await?;
    d.req("helen", "POST", &format!("/api/exhibitions/{exhibit}/publish"), json!({"expected_revision":revision}))
        .await?;
    Ok((exhibit, item))
}
pub async fn integration_recovery(d: &mut Driver) -> AppResult<()> {
    d.drain().await?; // Finish earlier deliveries so the outage affects exactly this story.
    d.req(
        "mark",
        "POST",
        "/api/admin/integrations/systems/content_manager",
        json!({"outage":true,"drop_responses":false}),
    )
    .await?;
    let (case, _) = d.submit("ben", "council-record-copy", json!({}), None).await?;
    d.action("olga", case, "cancel").await?;
    d.drain().await?;
    let deliveries = d.req("helen", "GET", &format!("/api/cases/{case}/integrations"), json!({})).await?;
    let delivery = deliveries
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["status"] == "failed")
        .ok_or_else(|| AppError::internal("Outage failure not visible"))?["id"]
        .as_i64()
        .unwrap();
    d.req(
        "mark",
        "POST",
        "/api/admin/integrations/systems/content_manager",
        json!({"outage":false,"drop_responses":false}),
    )
    .await?;
    d.req("mark", "POST", &format!("/api/admin/integrations/{delivery}/retry"), json!({})).await?;
    d.drain().await?;
    Ok(())
}
async fn clock_sweep(d: &Driver) -> AppResult<()> {
    crate::clock::scope(d.state.clock.clone(), crate::jobs::dispatch(&d.state, "deadline.sweep", &json!({}))).await
}
pub async fn verify(d: &mut Driver) -> AppResult<()> {
    let dashboard = d.req("helen", "GET", "/api/staff/dashboard", json!({})).await?;
    for metric in crate::records::metrics::METRICS {
        let list = d.req("helen", "GET", &format!("/api/staff/dashboard/metrics/{metric}"), json!({})).await?;
        if dashboard["metrics"][metric].as_u64() != Some(list["items"].as_array().unwrap().len() as u64) {
            return Err(AppError::internal(format!("Dashboard mismatch: {metric}")));
        }
    }
    let journal = d.req("tom", "GET", "/api/finance/ledger", json!({})).await?;
    let balance: i64 =
        journal["trial_balance"].as_array().unwrap().iter().map(|v| v["balance_cents"].as_i64().unwrap()).sum();
    if balance != 0 {
        return Err(AppError::internal("Seed ledger does not balance"));
    }
    let unbalanced:i64=sqlx::query_scalar("SELECT COUNT(*) FROM (SELECT entry_id FROM journal_lines GROUP BY entry_id HAVING SUM(debit_cents-credit_cents)<>0)").fetch_one(&d.state.db).await?;
    if unbalanced != 0 {
        return Err(AppError::internal("Unbalanced seed journal entry"));
    }
    Ok(())
}
/// Stage B compliance declaration: intake returns the unsigned declaration, the builder uploads version 2,
/// the inspection fee is paid, the site is inspected and written permission is issued against version 2.
pub async fn stage_notice(d: &mut Driver, project: &str, org: i64) -> AppResult<i64> {
    let (stage, docs) =
        d.submit("ben", "builder-stage-b-notice", json!({"project_reference":project}), Some(org)).await?;
    let declaration = &docs["declaration"];
    let revision = d.revision("olga", stage).await?;
    let comment = d.req("olga", "POST", &format!("/api/document-versions/{}/comments", declaration["version_id"]), json!({"expected_revision":revision,"body":"The declaration is not signed. Please upload the signed Stage B notice.","visibility":"applicant","request_new_version":true})).await?["id"].clone();
    let pdf = crate::pdf::simple_document(
        "Fictional Builder's Stage B compliance declaration (signed)",
        &[],
        &[("Declaration", "Signed by the fictional builder of Island Builders.".into())],
    );
    let signed = d
        .upload(
            "ben",
            &format!("/api/documents/{}/versions", declaration["id"]),
            &[("resolves_comment_ids", json!([comment]).to_string()), ("note", "Signed declaration".into())],
            &pdf,
        )
        .await?["version_id"]
        .as_i64()
        .unwrap();
    d.action("olga", stage, "advance").await?;
    d.pay("ben", stage, false).await?;
    d.complete_task(stage, "site_inspection").await?;
    d.decision(stage, "service_response", Some(vec![signed])).await?;
    Ok(stage)
}
pub async fn pipeline_crossing(d: &mut Driver) -> AppResult<i64> {
    let (case, _) = d.submit("ben", "pipeline-conduit-crossing", json!({"road_name":"Taylors Road","pipe_size_type":"Fictional 50 mm PE water main in 100 mm conduit","adjacent_portions":"Portion DEMO-44 and DEMO-45"}), None).await?;
    d.action("olga", case, "advance").await?;
    d.action("olga", case, "advance").await?;
    d.decision(case, "service_response", None).await?;
    Ok(case)
}
