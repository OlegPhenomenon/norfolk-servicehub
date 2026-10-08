//! Audit 2, N-04 (every Builder option executes) and N-06 (modify-approval form fidelity, `group` fields).
mod support;
use serde_json::{Value, json};
use servicehub::seed::driver::Driver;

async fn req(d: &mut Driver, who: &str, method: &str, path: &str, body: Value) -> Value {
    d.req(who, method, path, body).await.unwrap()
}
async fn rev(d: &mut Driver, who: &str, c: i64) -> i64 {
    d.revision(who, c).await.unwrap()
}
/// Creates a service through the Builder API, saves `def`, requires a clean validation and publishes it.
async fn publish(d: &mut Driver, slug: &str, module: &str, def: Value) -> i64 {
    let created = req(
        d,
        "mark",
        "POST",
        "/api/admin/services",
        json!({"slug":slug,"name":format!("Audit probe {slug}"),"category":"Testing","department":"Customer Care","module":module}),
    )
    .await;
    let base = format!("/api/admin/services/{}/versions/{}", created["id"], created["version_id"]);
    req(d, "mark", "PUT", &base, def).await;
    assert_eq!(req(d, "mark", "POST", &format!("{base}/validate"), json!({})).await["issues"], json!([]), "{slug}");
    req(d, "mark", "POST", &format!("{base}/publish"), json!({})).await;
    created["id"].as_i64().unwrap()
}
async fn advance_blocked(d: &mut Driver, who: &str, c: i64, slug: &str) -> String {
    let detail = d.detail(who, c).await.unwrap();
    assert_eq!(detail["case"]["current_step"], "probe", "{slug}");
    let reason = detail["guard_reason"]
        .as_str()
        .unwrap_or_else(|| panic!("{slug}: the probe step must block with a reason"))
        .to_owned();
    let r = rev(d, who, c).await;
    let body = d
        .expect(
            who,
            "POST",
            &format!("/api/cases/{c}/actions/advance"),
            json!({"expected_revision":r,"reason":"Too early"}),
            409,
        )
        .await
        .unwrap();
    assert_eq!(body["error"]["message"], reason.as_str(), "{slug}");
    reason
}
async fn issue_letter(d: &mut Driver, who: &str, c: i64, letter_type: &str) {
    let letters = req(d, who, "GET", &format!("/api/cases/{c}/letters"), json!({})).await;
    assert_eq!(letters["can_issue"], true);
    assert!(
        letters["steps"].as_array().unwrap().iter().any(|s| s["letter_type"] == letter_type && s["current"] == true)
    );
    let r = rev(d, who, c).await;
    req(
        d,
        who,
        "POST",
        &format!("/api/cases/{c}/letters"),
        json!({"letter_type":letter_type,"title":"Fictional response","body":"Council has considered the request.","expected_revision":r}),
    )
    .await;
}
fn probe_step(role: &str, extra: Value) -> Value {
    let mut step = json!({"key":"probe","kind":"module","role":role,"label":"Probe","applicant_label":"Probe step"});
    for (k, v) in extra.as_object().unwrap() {
        step[k] = v.clone();
    }
    step
}

/// For every module: each offered handler, task kind and decision type is published in a Builder-created
/// definition (probe step right after the first step), blocks with a human reason (never an internal error) and,
/// for every option a case can satisfy at that point, passes after the staff action. Module checkpoints whose
/// prerequisites only exist later in the module's own flow must already be part of that module's seeded
/// workflow, which `acceptance_scenarios.rs` drives to completion.
#[tokio::test]
async fn every_builder_option_executes_in_every_module() {
    let (mut d, _dir) = support::fixture().await;
    let ruth = d.people["ruth"].user_id;
    req(
        &mut d,
        "helen",
        "POST",
        "/api/staff/decision-authorities",
        json!({"user_id":ruth,"decision_type":"service_response"}),
    )
    .await;
    let bases = [
        ("generic", "council-record-copy", "olga"),
        ("venue_booking", "rawson-hall-hire", "olga"),
        ("equipment_hire", "equipment-hire", "olga"),
        ("building", "development-application", "olga"),
        ("planning_certificate", "planning-certificate", "olga"),
        ("road_issue", "road-issue", "olga"),
        ("complaint", "complaint", "ruth"),
    ];
    let mut n = 0;
    for (module, base_slug, staff) in bases {
        let caps = req(&mut d, "mark", "GET", &format!("/api/admin/services/capabilities/{module}"), json!({})).await;
        let base =
            req(&mut d, "alexey", "GET", &format!("/api/public/services/{base_slug}"), json!({})).await["definition"]
                .clone();
        let base_handlers: Vec<&str> =
            base["workflow"]["steps"].as_array().unwrap().iter().filter_map(|s| s["handler"].as_str()).collect();
        let base_tasks: Vec<&str> =
            base["workflow"]["steps"].as_array().unwrap().iter().filter_map(|s| s["task_kind"].as_str()).collect();
        let role = if module == "complaint" { "complaints_officer" } else { "intake" };
        let mut probes = vec![];
        for h in caps["handlers"].as_array().unwrap() {
            probes.push(probe_step(role, json!({"handler":h})));
        }
        for k in caps["task_kinds"].as_array().unwrap() {
            probes.push(probe_step(role, json!({"kind":"task","task_kind":k})));
        }
        for t in caps["decision_types"].as_array().unwrap() {
            probes.push(probe_step(role, json!({"kind":"decision","decision_types":[t]})));
        }
        assert_eq!(caps["step_kinds"].as_array().unwrap().contains(&json!("task")), module != "complaint");
        for step in probes {
            n += 1;
            let slug = format!("probe-{}-{n}", module.replace('_', "-"));
            let mut def = base.clone();
            def["workflow"]["steps"].as_array_mut().unwrap().insert(1, step.clone());
            if step["decision_types"] == json!(["modification_approval"]) {
                // Only meaningful with an `original_approval` field (the approval being modified): rejected at
                // publish otherwise, and it is the decision step of the seeded modification service.
                let created = req(&mut d,"mark","POST","/api/admin/services",json!({"slug":slug,"name":"Modification probe","category":"Testing","department":"Planning","module":module})).await;
                let path = format!("/api/admin/services/{}/versions/{}", created["id"], created["version_id"]);
                req(&mut d, "mark", "PUT", &path, def).await;
                let issues = req(&mut d, "mark", "POST", &format!("{path}/validate"), json!({})).await;
                assert!(
                    issues["issues"].as_array().unwrap().iter().any(|i| i["path"] == "workflow.steps.1.decision_types")
                );
                let modify = req(&mut d, "alexey", "GET", "/api/public/services/modify-approval", json!({})).await;
                assert!(
                    modify["definition"]["workflow"]["steps"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|s| s["decision_types"] == json!(["modification_approval"]))
                );
                continue;
            }
            publish(&mut d, &slug, module, def).await;
            let extra = match module {
                "venue_booking" => d.hall("rawson-main", 30 + n * 2),
                "equipment_hire" => {
                    json!({"request":{"description":"Probe excavation","requested_hours":2,"preferred_date":"2026-10-20","site_text":"Fictional depot"}})
                }
                _ => json!({}),
            };
            let (c, _) = d.submit("alexey", &slug, extra, None).await.unwrap();
            d.action(staff, c, "advance").await.unwrap();
            // Module checkpoints (booking, bond, equipment, exhibition) evaluate their own state; out of their
            // canonical position they may already pass (e.g. no bond invoiced yet) but must never error.
            let checkpoint = step["handler"].as_str().is_some_and(|h| !h.starts_with("documents.letter_issued:"))
                || step["task_kind"] == "equipment_job";
            if checkpoint {
                let detail = d.detail(staff, c).await.unwrap();
                assert!(detail["guard_reason"].is_null() || detail["guard_reason"].is_string(), "{slug}");
            } else {
                let reason = advance_blocked(&mut d, staff, c, &slug).await;
                assert!(!reason.contains("Unknown"), "{slug}: {reason}");
            }
            match (step["kind"].as_str().unwrap(), step["handler"].as_str(), step["task_kind"].as_str()) {
                ("module", Some(h), _) if h.starts_with("documents.letter_issued:") => {
                    issue_letter(&mut d, staff, c, h.trim_start_matches("documents.letter_issued:")).await;
                    let docs = req(&mut d, "alexey", "GET", &format!("/api/cases/{c}/documents"), json!({})).await;
                    assert!(docs.as_array().unwrap().iter().any(|doc| doc["category"] == "letter"), "{slug}");
                }
                ("module", Some(h), _) => {
                    assert!(base_handlers.contains(&h), "{slug}: {h} must be a checkpoint of the module flow");
                    continue;
                }
                ("task", _, Some("equipment_job")) => {
                    assert!(base_tasks.contains(&"equipment_job"));
                    continue;
                }
                ("task", _, Some(kind)) => d.complete_task(c, kind).await.unwrap(),
                ("decision", _, _) => {
                    let t = step["decision_types"][0].as_str().unwrap();
                    if module == "complaint" {
                        let templates = req(&mut d, "helen", "GET", "/api/decision-templates", json!({})).await;
                        let template = templates.as_array().unwrap().iter().find(|x| x["decision_type"] == t).unwrap()
                            ["id"]
                            .clone();
                        let r = rev(&mut d, "helen", c).await;
                        let id = req(&mut d,"helen","POST",&format!("/api/cases/{c}/decisions"),json!({"decision_type":t,"outcome":"approved","reasons":"Fictional complaint result.","conditions":"","template_id":template,"evidence_version_ids":[],"expected_revision":r})).await["id"].clone();
                        for (who, action) in [("helen", "submit"), ("ruth", "issue")] {
                            let r = rev(&mut d, who, c).await;
                            req(
                                &mut d,
                                who,
                                "POST",
                                &format!("/api/cases/{c}/decisions/{id}/{action}"),
                                json!({"expected_revision":r}),
                            )
                            .await;
                        }
                    } else {
                        d.decision(c, t, None).await.unwrap();
                    }
                }
                other => panic!("unexpected probe {other:?}"),
            }
            let after = d.detail(staff, c).await.unwrap();
            assert_ne!(after["case"]["current_step"], "probe", "{slug} did not pass after the action");
        }
    }
    // A complaint task step is not offered and is rejected on publish (field workers never see complaints).
    let created = req(&mut d,"mark","POST","/api/admin/services",json!({"slug":"probe-complaint-task","name":"Complaint task","category":"Testing","department":"Governance","module":"complaint"})).await;
    let base = format!("/api/admin/services/{}/versions/{}", created["id"], created["version_id"]);
    let mut def = req(&mut d, "alexey", "GET", "/api/public/services/complaint", json!({})).await["definition"].clone();
    def["workflow"]["steps"]
        .as_array_mut()
        .unwrap()
        .insert(1, probe_step("complaints_officer", json!({"kind":"task","task_kind":"general"})));
    req(&mut d, "mark", "PUT", &base, def).await;
    let issues = req(&mut d, "mark", "POST", &format!("{base}/validate"), json!({})).await;
    assert!(issues["issues"].as_array().unwrap().iter().any(|i| i["path"] == "workflow.steps.1.task_kind"));
}

/// The audit's acceptance path: a brand-new generic service with a service-response letter step and an ordinary
/// field task; the letter is refused for services that do not wait for it; the task is reassigned to another
/// field worker and completed.
#[tokio::test]
async fn new_service_letter_step_and_reassigned_field_task() {
    let (mut d, _dir) = support::fixture().await;
    let def = json!({"module":"generic","summary":"Fictional garden access","outcome":"A written response","fields":[{"key":"purpose","type":"text","label":"Purpose","required":true}],"documents":[],"workflow":{"steps":[
        {"key":"intake","kind":"review","role":"intake","label":"Check request","applicant_label":"Checking"},
        {"key":"visit","kind":"task","task_kind":"general","role":"intake","label":"Site visit","applicant_label":"Site visit"},
        {"key":"reply","kind":"module","handler":"documents.letter_issued:service_response","role":"intake","label":"Write response","applicant_label":"Preparing your response"},
        {"key":"done","kind":"complete","label":"Complete","applicant_label":"Complete"}]},"deadlines":[],"pricing":[]});
    publish(&mut d, "garden-access", "generic", def).await;
    let (c, _) = d.submit("alexey", "garden-access", json!({"purpose":"Vegetables"}), None).await.unwrap();
    // A letter the workflow does not wait for is rejected; so is one for another service's workflow.
    let r = rev(&mut d, "olga", c).await;
    d.expect(
        "olga",
        "POST",
        &format!("/api/cases/{c}/letters"),
        json!({"letter_type":"road_response","title":"x","body":"y","expected_revision":r}),
        422,
    )
    .await
    .unwrap();
    d.action("olga", c, "advance").await.unwrap();
    let tasks = req(&mut d, "olga", "GET", &format!("/api/cases/{c}/tasks"), json!({})).await;
    let task = tasks[0].clone();
    assert_eq!(task["kind"], "general");
    assert_eq!(task["assigned_to"], d.people["jake"].user_id);
    let jake_list = req(&mut d, "jake", "GET", "/api/field/tasks", json!({})).await;
    assert!(jake_list.as_array().unwrap().iter().any(|t| t["id"] == task["id"]));
    // A second field worker created through the admin API, then assigned from the case Tasks panel endpoint.
    let worker = req(&mut d,"mark","POST","/api/admin/users",json!({"email":"second.worker@example.invalid","display_name":"Fictional Second Worker","kind":"staff","password":"fictional-worker-password","job_title":"Field worker"})).await["id"].as_i64().unwrap();
    req(&mut d, "mark", "POST", &format!("/api/admin/users/{worker}/roles"), json!({"role":"field_worker"})).await;
    req(
        &mut d,
        "olga",
        "POST",
        &format!("/api/tasks/{}/assign", task["id"]),
        json!({"assigned_to":worker,"expected_revision":task["revision"]}),
    )
    .await;
    let moved = req(&mut d, "olga", "GET", &format!("/api/cases/{c}/tasks"), json!({})).await[0].clone();
    assert_eq!(moved["assigned_to"], worker);
    assert!(
        !req(&mut d, "jake", "GET", "/api/field/tasks", json!({}))
            .await
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == task["id"])
    );
    let jake = d.people["jake"].user_id;
    req(
        &mut d,
        "olga",
        "POST",
        &format!("/api/tasks/{}/assign", task["id"]),
        json!({"assigned_to":jake,"expected_revision":moved["revision"]}),
    )
    .await;
    d.complete_task(c, "general").await.unwrap();
    let detail = d.detail("olga", c).await.unwrap();
    assert_eq!(detail["case"]["current_step"], "reply");
    assert!(detail["guard_reason"].as_str().unwrap().contains("service response"));
    let r = rev(&mut d, "olga", c).await;
    d.expect(
        "olga",
        "POST",
        &format!("/api/cases/{c}/actions/advance"),
        json!({"expected_revision":r,"reason":"Early"}),
        409,
    )
    .await
    .unwrap();
    let r = rev(&mut d, "olga", c).await;
    req(&mut d,"olga","POST",&format!("/api/cases/{c}/letters"),json!({"letter_type":"service_response","title":"Garden access response","body":"Access is granted on weekdays.","expected_revision":r})).await;
    let detail = d.detail("alexey", c).await.unwrap();
    assert_eq!(detail["case"]["status"], "completed");
    let docs = req(&mut d, "alexey", "GET", &format!("/api/cases/{c}/documents"), json!({})).await;
    assert!(
        docs.as_array()
            .unwrap()
            .iter()
            .any(|doc| doc["category"] == "letter" && doc["title"] == "Garden access response")
    );
    let letters = req(&mut d, "olga", "GET", &format!("/api/cases/{c}/letters"), json!({})).await;
    assert_eq!(letters["issued"][0]["letter_type"], "service_response");
    assert_eq!(letters["can_issue"], false);
}

/// Builder validation of `group` fields and multiselect `show_if` contains semantics in a Builder-made service.
#[tokio::test]
async fn group_fields_are_validated_in_definitions_and_answers() {
    let (mut d, _dir) = support::fixture().await;
    let created = req(&mut d,"mark","POST","/api/admin/services",json!({"slug":"group-probe","name":"Group probe","category":"Testing","department":"Customer Care","module":"generic"})).await;
    let base = format!("/api/admin/services/{}/versions/{}", created["id"], created["version_id"]);
    let steps = json!({"steps":[{"key":"intake","kind":"review","role":"intake","label":"Check","applicant_label":"Checking"},{"key":"done","kind":"complete","label":"Done","applicant_label":"Done"}]});
    let bad = json!({"module":"generic","summary":"s","outcome":"o","fields":[
        {"key":"people","type":"group","label":"People","columns":[]},
        {"key":"spots","type":"group","label":"Spots","min_items":3,"max_items":1,"columns":[{"key":"where","type":"location","label":"Where"},{"key":"where","type":"text","label":"Again"}]},
        {"key":"plain","type":"text","label":"Plain","columns":[{"key":"a","type":"text","label":"A"}]}],"documents":[],"workflow":steps.clone()});
    req(&mut d, "mark", "PUT", &base, bad).await;
    let issues = req(&mut d, "mark", "POST", &format!("{base}/validate"), json!({})).await["issues"].clone();
    let paths: Vec<&str> = issues.as_array().unwrap().iter().map(|i| i["path"].as_str().unwrap()).collect();
    for p in [
        "fields.0.columns",
        "fields.1.min_items",
        "fields.1.columns.0.type",
        "fields.1.columns.1.key",
        "fields.2.columns",
    ] {
        assert!(paths.contains(&p), "{p} missing from {paths:?}");
    }
    let good = json!({"module":"generic","summary":"s","outcome":"o","fields":[
        {"key":"kinds","type":"multiselect","label":"Kinds","required":true,"options":[{"value":"a","label":"A"},{"value":"b","label":"B"}]},
        {"key":"b_details","type":"text","label":"B details","required":true,"show_if":{"field":"kinds","equals":"b"}},
        {"key":"people","type":"group","label":"People","required":true,"max_items":2,"columns":[
            {"key":"name","type":"text","label":"Name","required":true},{"key":"age","type":"number","label":"Age","min":0},
            {"key":"agree","type":"checkbox","label":"Agrees","required":true},{"key":"role","type":"select","label":"Role","options":[{"value":"owner","label":"Owner"}]}]}],
        "documents":[],"workflow":steps});
    req(&mut d, "mark", "PUT", &base, good).await;
    assert_eq!(req(&mut d, "mark", "POST", &format!("{base}/validate"), json!({})).await["issues"], json!([]));
    let preview = |answers: Value| json!({"answers":answers});
    let r = req(&mut d,"mark","POST",&format!("{base}/preview-answers"),preview(json!({"kinds":["a","b"],"people":[{"name":"Ann","agree":false,"age":-1,"role":"boss"},"x",{"name":"","agree":true}]}))).await;
    assert_eq!(r["valid"], false);
    for k in ["b_details", "people", "people.0.agree", "people.0.age", "people.0.role", "people.1", "people.2.name"] {
        assert!(r["fields"].get(k).is_some(), "{k} missing from {}", r["fields"]);
    }
    let r =
        req(&mut d, "mark", "POST", &format!("{base}/preview-answers"), preview(json!({"kinds":["a"],"people":[]})))
            .await;
    assert_eq!(r["fields"]["people"], "Add at least one row.");
    assert!(r["fields"].get("b_details").is_none());
    req(&mut d, "mark", "POST", &format!("{base}/publish"), json!({})).await;
    let (c, _) = d
        .submit("alexey", "group-probe", json!({"kinds":["b","a"],"b_details":"Shown by contains","people":[{"name":"Ann","agree":true,"unknown":"dropped"},{"name":"Bo","agree":true,"age":7}]}), None)
        .await
        .unwrap();
    let answers = d.detail("olga", c).await.unwrap()["answers"].clone();
    assert_eq!(answers["people"], json!([{"name":"Ann","agree":true},{"name":"Bo","agree":true,"age":7}]));
    assert_eq!(answers["b_details"], "Shown by contains");
}

/// N-06: two applicants and landowners, three parcels, two modification types incl. a proposed lapse date; staff
/// see every answer and the document list with requiredness basis, hidden answers are dropped.
#[tokio::test]
async fn modify_approval_captures_every_form_section() {
    let (mut d, _dir) = support::fixture().await;
    let org = req(&mut d, "ben", "GET", "/api/my/organisations", json!({})).await[0]["id"].as_i64().unwrap();
    let (da, _) = d.submit("ben", "development-application", json!({}), Some(org)).await.unwrap();
    d.action("olga", da, "advance").await.unwrap();
    d.action("priya", da, "advance").await.unwrap();
    d.action("priya", da, "skip").await.unwrap();
    d.decision(da, "development_approval", None).await.unwrap();
    let approval = d.decision(da, "building_approval", None).await.unwrap();

    let form = req(&mut d, "ben", "GET", "/api/public/services/modify-approval", json!({})).await["definition"].clone();
    let keys: Vec<&str> = form["fields"].as_array().unwrap().iter().map(|f| f["key"].as_str().unwrap()).collect();
    for key in [
        "original_approval",
        "applicants",
        "landowners_are_applicants",
        "landowners",
        "property_ref",
        "parcels",
        "land_tenure",
        "zoning",
        "current_use",
        "use_types",
        "modification_types",
        "minor_error_description",
        "conditions_description",
        "proposed_lapse_date",
        "lapse_date_reasons",
        "other_modification_description",
        "modified_proposal",
        "external_environment_changes",
        "estimated_cost",
        "other_approvals",
        "declaration",
    ] {
        assert!(keys.contains(&key), "{key}");
    }
    for old in ["applicant_name", "owner_name", "modification_type", "lot", "section", "substantially_same"] {
        assert!(!keys.contains(&old), "{old} must be replaced");
    }
    let docs: Vec<(&str, bool)> = form["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["key"].as_str().unwrap(), d["required"].as_bool().unwrap()))
        .collect();
    assert_eq!(
        docs,
        [("title_search", true), ("owners_consent", true), ("modification_plans", true), ("supporting", false)]
    );
    assert!(
        form["documents"].as_array().unwrap().iter().all(|d| d["help"].as_str().is_some_and(|h| h.contains("Section")))
    );

    let person = |first: &str, last: &str| json!({"first_name":first,"last_name":last,"postal_address":"PO Box 1, Norfolk Island 2899","phone":"+672 3 22001","mobile":"+672 5 12345","email":format!("{}@example.invalid", first.to_lowercase())});
    let mut owner_a = person("Olive", "Owner");
    owner_a["consent"] = json!(true);
    let mut owner_b = person("Oscar", "Owner");
    owner_b["consent"] = json!(false);
    let mut answers = json!({
        "original_approval":{"decision_id":approval},
        "applicants":[person("Ben","Carter"),person("Bea","Carter")],
        "landowners_are_applicants":"no",
        "landowners":[owner_a,owner_b],
        "property_ref":"44 Taylors Road, Burnt Pine",
        "parcels":[{"portion":"44h","lot":"1","section":"9","land_area":"2,000 m²"},{"portion":"44j","lot":"2","section":"9"},{"portion":"45"}],
        "land_tenure":"Freehold","zoning":"Rural","current_use":"Dwelling house",
        "use_types":["alterations_additions","other"],"use_types_other":"Water tank enclosure",
        "modification_types":["conditions","lapse_date"],
        "conditions_description":"Condition 4 asks for a 20,000 L tank; request 15,000 L.",
        "minor_error_description":"Hidden answer must be dropped",
        "lapse_date_reasons":"Builder availability",
        "modified_proposal":"Same dwelling with a smaller tank","external_environment_changes":"None",
        "estimated_cost":45000,"other_approvals":["trees"],"declaration":true});
    let c =
        req(&mut d, "ben", "POST", "/api/services/modify-approval/drafts", json!({"applicant_org_id":org})).await["id"]
            .as_i64()
            .unwrap();
    req(&mut d, "ben", "PUT", &format!("/api/cases/{c}/draft"), json!({"answers":answers})).await;
    let pdf = servicehub::pdf::simple_document("Fictional attachment", &[], &[("Body", "Fictional".into())]);
    for (key, title) in [
        ("title_search", "Title search"),
        ("owners_consent", "Signed landowner consent"),
        ("modification_plans", "Impact statement and plans"),
    ] {
        d.upload(
            "ben",
            &format!("/api/cases/{c}/documents"),
            &[("requirement_key", key.into()), ("title", title.into())],
            &pdf,
        )
        .await
        .unwrap();
    }
    let refused = d.expect("ben", "POST", &format!("/api/cases/{c}/submit"), json!({}), 422).await.unwrap();
    for key in ["landowners.1.consent", "landowners", "proposed_lapse_date"] {
        assert!(refused["error"]["fields"].get(key).is_some(), "{key} missing from {}", refused["error"]["fields"]);
    }
    answers["landowners"][1]["consent"] = json!(true);
    answers["proposed_lapse_date"] = json!("2027-06-30");
    req(&mut d, "ben", "PUT", &format!("/api/cases/{c}/draft"), json!({"answers":answers})).await;
    req(&mut d, "ben", "POST", &format!("/api/cases/{c}/submit"), json!({})).await;

    let staff = d.detail("priya", c).await.unwrap();
    let a = &staff["answers"];
    assert_eq!(a["applicants"].as_array().unwrap().len(), 2);
    assert_eq!(a["applicants"][1]["email"], "bea@example.invalid");
    assert_eq!(a["landowners"].as_array().unwrap().len(), 2);
    assert_eq!(a["parcels"].as_array().unwrap().len(), 3);
    assert_eq!(a["parcels"][0]["land_area"], "2,000 m²");
    assert_eq!(a["modification_types"], json!(["conditions", "lapse_date"]));
    assert_eq!(a["proposed_lapse_date"], "2027-06-30");
    assert!(a.get("minor_error_description").is_none());
    let panel = req(&mut d, "priya", "GET", &format!("/api/cases/{c}/decisions"), json!({})).await;
    assert!(
        panel["document_requirements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["key"] == "owners_consent" && r["help"].as_str().unwrap().contains("every landowner"))
    );
    // Group answers are searchable by staff.
    let found = req(&mut d, "priya", "GET", "/api/staff/cases?q=Oscar", json!({})).await;
    assert!(found["items"].as_array().unwrap().iter().any(|i| i["id"] == c));
}
