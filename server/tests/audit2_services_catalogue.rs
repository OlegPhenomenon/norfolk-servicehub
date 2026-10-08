//! Audit 2, N-05 and the catalogue upgrade: Stage A–E notices on a building project, form 212 pipeline
//! crossings, builder-created follow-up services, and versioned upgrades of an installed catalogue.
// Shared with the other HTTP suites; this suite does not need the fixture directory path.
#[allow(dead_code)]
mod support;
use serde_json::{Value, json};
use servicehub::{
    seed::{driver::Driver, scenarios},
    services::{self, CatalogueAction},
};

async fn scalar(d: &Driver, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&d.state.db).await.unwrap()
}
async fn ben_org(d: &Driver) -> i64 {
    scalar(d, "SELECT organisation_id FROM memberships WHERE user_id=(SELECT id FROM users WHERE persona_key='ben') AND status='active'").await
}
/// A new building project: submitting a development application creates it.
async fn new_project(d: &mut Driver, org: i64) -> i64 {
    let (case, _) = d.submit("ben", "development-application", json!({}), Some(org)).await.unwrap();
    scalar(d, &format!("SELECT building_project_id FROM cases WHERE id={case}")).await
}
async fn upgrade(d: &Driver) -> Vec<services::CatalogueChange> {
    let mut tx = servicehub::db::write_tx(&d.state.db).await.unwrap();
    let changes = services::upgrade(&mut tx, &d.state).await.unwrap();
    tx.commit().await.unwrap();
    changes
}
fn action<'a>(changes: &'a [services::CatalogueChange], slug: &str) -> &'a services::CatalogueChange {
    changes.iter().find(|c| c.slug == slug).unwrap()
}

#[tokio::test]
async fn catalogue_offers_stage_a_to_e_and_pipeline_crossing() {
    let (mut d, _dir) = support::fixture().await;
    let items = d.req("alexey", "GET", "/api/public/services", json!({})).await.unwrap()["items"].clone();
    let slugs: Vec<&str> = items.as_array().unwrap().iter().map(|s| s["slug"].as_str().unwrap()).collect();
    for stage in ["a", "b", "c", "d", "e"] {
        assert!(slugs.contains(&format!("builder-stage-{stage}-notice").as_str()), "{slugs:?}");
        let def = d
            .req("alexey", "GET", &format!("/api/public/services/builder-stage-{stage}-notice"), json!({}))
            .await
            .unwrap()["definition"]
            .clone();
        assert_eq!(def["building_role"], "follow_up");
        assert_eq!(def["fields"][0]["type"], "project_ref");
        assert!(
            def["documents"].as_array().unwrap().iter().any(|d| d["key"] == "declaration" && d["required"] == true)
        );
        let steps: Vec<&str> =
            def["workflow"]["steps"].as_array().unwrap().iter().map(|s| s["key"].as_str().unwrap()).collect();
        assert_eq!(steps, ["intake", "payment", "site", "decision", "done"]);
        assert_eq!(def["pricing"][0]["item"], "BUILDING_STAGE_INSPECTION");
    }
    let stage_a =
        d.req("alexey", "GET", "/api/public/services/builder-stage-a-notice", json!({})).await.unwrap()["definition"]
            .clone();
    assert_eq!(
        stage_a["fields"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["key"].as_str().unwrap().starts_with("stage_item_"))
            .count(),
        3
    );
    assert!(slugs.contains(&"pipeline-conduit-crossing"));
    let pipe = d.req("alexey", "GET", "/api/public/services/pipeline-conduit-crossing", json!({})).await.unwrap();
    assert_eq!(pipe["service"]["department"], "Works Depot");
    let def = &pipe["definition"];
    for key in [
        "applicant_name",
        "postal_address",
        "email",
        "phone",
        "abn",
        "road_name",
        "crossing_location",
        "pipe_size_type",
        "adjacent_portions",
        "no_work",
        "restoration",
    ] {
        assert!(def["fields"].as_array().unwrap().iter().any(|f| f["key"] == key && f["required"] == true), "{key}");
    }
    assert_eq!(def["documents"][0]["key"], "drawing");
    assert_eq!(def["documents"][0]["required"], true);
    assert_eq!(def["pricing"], json!([]));
    assert!(def["price_note"].as_str().unwrap().contains("Council will confirm"));
    assert_eq!(def["deadlines"][1]["days"], 10);
    let search = d.req("alexey", "GET", "/api/public/services?q=conduit", json!({})).await.unwrap();
    assert!(search["items"].as_array().unwrap().iter().any(|s| s["slug"] == "pipeline-conduit-crossing"));
}

#[tokio::test]
async fn stage_notice_is_returned_replaced_accepted_and_visible_in_project_history() {
    let (mut d, _dir) = support::fixture().await;
    let org = ben_org(&d).await;
    let project = new_project(&mut d, org).await;
    // The applicant picks the project from the projects they can access; others do not see it.
    let mine = d.req("ben", "GET", "/api/my/building-projects", json!({})).await.unwrap();
    let reference = mine.as_array().unwrap().iter().find(|p| p["id"] == project).unwrap()["reference"]
        .as_str()
        .unwrap()
        .to_string();
    let theirs = d.req("alexey", "GET", "/api/my/building-projects", json!({})).await.unwrap();
    assert!(theirs.as_array().unwrap().iter().all(|p| p["id"] != project));
    let draft = d.req("alexey", "POST", "/api/services/builder-stage-c-notice/drafts", json!({})).await.unwrap()["id"]
        .as_i64()
        .unwrap();
    let (_, answers) = d.answers("alexey", "builder-stage-c-notice").await.unwrap();
    let mut answers = answers;
    answers["project_reference"] = json!(reference);
    d.req("alexey", "PUT", &format!("/api/cases/{draft}/draft"), json!({"answers":answers})).await.unwrap();
    let pdf = servicehub::pdf::simple_document("Fictional declaration", &[], &[]);
    d.upload("alexey", &format!("/api/cases/{draft}/documents"), &[("requirement_key", "declaration".into())], &pdf)
        .await
        .unwrap();
    let denied = d.expect("alexey", "POST", &format!("/api/cases/{draft}/submit"), json!({}), 422).await.unwrap();
    assert!(denied.to_string().contains("project_reference"), "{denied}");

    let (stage, docs) =
        d.submit("ben", "builder-stage-b-notice", json!({"project_reference":reference}), Some(org)).await.unwrap();
    assert_eq!(scalar(&d, &format!("SELECT building_project_id FROM cases WHERE id={stage}")).await, project);
    assert_eq!(
        scalar(&d, &format!("SELECT COUNT(*) FROM case_links WHERE from_case_id={stage} AND kind='follow_up_of'"))
            .await,
        1
    );
    // Staff return the unsigned declaration for a new version: the request waits for the applicant.
    let declaration = docs["declaration"].clone();
    let revision = d.revision("olga", stage).await.unwrap();
    let comment = d.req("olga", "POST", &format!("/api/document-versions/{}/comments", declaration["version_id"]), json!({"expected_revision":revision,"body":"Please upload the signed declaration.","visibility":"applicant","request_new_version":true})).await.unwrap()["id"].clone();
    assert_eq!(d.detail("ben", stage).await.unwrap()["case"]["status"], "waiting_on_applicant");
    let revision = d.revision("olga", stage).await.unwrap();
    d.expect(
        "olga",
        "POST",
        &format!("/api/cases/{stage}/actions/advance"),
        json!({"expected_revision":revision}),
        403,
    )
    .await
    .unwrap();
    let signed = d
        .upload(
            "ben",
            &format!("/api/documents/{}/versions", declaration["id"]),
            &[("resolves_comment_ids", json!([comment]).to_string()), ("note", "Signed".into())],
            &pdf,
        )
        .await
        .unwrap()["version_id"]
        .as_i64()
        .unwrap();
    d.action("olga", stage, "advance").await.unwrap();
    // Inspection fee before the inspection; the inspection task is required.
    let money = d.money("ben", stage).await.unwrap();
    assert_eq!(money["invoices"][0]["outstanding_cents"], 8300);
    d.pay("ben", stage, false).await.unwrap();
    assert_eq!(d.detail("olga", stage).await.unwrap()["case"]["current_step"], "site");
    let revision = d.revision("olga", stage).await.unwrap();
    d.expect(
        "olga",
        "POST",
        &format!("/api/cases/{stage}/actions/skip"),
        json!({"expected_revision":revision,"reason":"No"}),
        403,
    )
    .await
    .unwrap();
    d.complete_task(stage, "site_inspection").await.unwrap();
    // A follow-up notice never receives an approval; acceptance is written permission against version 2.
    let templates = d.req("priya", "GET", "/api/decision-templates", json!({})).await.unwrap();
    let template =
        templates.as_array().unwrap().iter().find(|t| t["decision_type"] == "building_approval").unwrap()["id"].clone();
    let revision = d.revision("priya", stage).await.unwrap();
    d.expect("priya", "POST", &format!("/api/cases/{stage}/decisions"), json!({"decision_type":"building_approval","outcome":"approved","reasons":"Wrong type","conditions":"","template_id":template,"expected_revision":revision}), 422).await.unwrap();
    let list = d.req("priya", "GET", &format!("/api/cases/{stage}/decisions"), json!({})).await.unwrap();
    assert_eq!(list["allowed_decision_types"], json!(["service_response"]));
    d.decision(stage, "service_response", Some(vec![signed])).await.unwrap();
    assert_eq!(d.detail("ben", stage).await.unwrap()["case"]["status"], "completed");

    let history = d.req("ben", "GET", &format!("/api/building-projects/{project}"), json!({})).await.unwrap();
    let case = history["cases"].as_array().unwrap().iter().find(|c| c["id"] == stage).unwrap();
    let versions =
        case["documents"].as_array().unwrap().iter().find(|doc| doc["id"] == declaration["id"]).unwrap()["versions"]
            .clone();
    assert_eq!(versions.as_array().unwrap().len(), 2);
    assert_eq!(versions[0]["comments"][0]["request_new_version"], true);
    assert_eq!(versions[0]["comments"][0]["resolved_by_version_id"], signed);
    let accepted = history["decisions"].as_array().unwrap().iter().find(|d| d["case_id"] == stage).unwrap();
    assert_eq!(accepted["decision_type"], "service_response");
    assert_eq!(accepted["status"], "issued");
    assert_eq!(accepted["evidence"][0]["version"], 2);
    let events: Vec<&str> = history["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["case_id"] == stage)
        .map(|e| e["summary"].as_str().unwrap())
        .collect();
    assert!(events.iter().any(|e| e.contains("version 2")), "{events:?}");
}

#[tokio::test]
async fn seeded_stage_story_and_pipeline_crossing_reach_their_decisions() {
    let (mut d, _dir) = support::fixture().await;
    let org = ben_org(&d).await;
    let project = new_project(&mut d, org).await;
    let stage = scenarios::stage_notice(&mut d, &project.to_string(), org).await.unwrap();
    assert_eq!(d.detail("ben", stage).await.unwrap()["case"]["status"], "completed");
    let pipe = scenarios::pipeline_crossing(&mut d).await.unwrap();
    let detail = d.detail("ben", pipe).await.unwrap();
    assert_eq!(detail["case"]["status"], "completed");
    assert_eq!(scalar(&d, &format!("SELECT COUNT(*) FROM decisions WHERE case_id={pipe} AND decision_type='service_response' AND status='issued'")).await, 1);
    assert!(scalar(&d, &format!("SELECT COUNT(*) FROM notifications WHERE case_id={pipe} AND user_id=(SELECT id FROM users WHERE persona_key='ben')")).await > 0);
}

#[tokio::test]
async fn builder_created_follow_up_service_links_to_its_project() {
    let (mut d, _dir) = support::fixture().await;
    let org = ben_org(&d).await;
    let project = new_project(&mut d, org).await;
    let service = d.req("mark","POST","/api/admin/services",json!({"slug":"retaining-wall-notice","name":"Fictional retaining wall notice","category":"Planning & Building","department":"Planning","module":"building"})).await.unwrap();
    let (id, v) = (service["id"].as_i64().unwrap(), service["version_id"].as_i64().unwrap());
    let steps = json!([{"key":"intake","kind":"review","role":"intake","label":"Check","applicant_label":"Checking"},{"key":"site","kind":"task","role":"field_worker","task_kind":"site_inspection","label":"Inspect","applicant_label":"Inspection"},{"key":"done","kind":"complete","label":"Done","applicant_label":"Done"}]);
    let mut def = json!({"module":"building","building_role":"follow_up","summary":"Fictional notice.","outcome":"Recorded on the project.","fields":[{"key":"wall","type":"text","label":"Wall","required":true}],"documents":[],"workflow":{"steps":steps},"deadlines":[],"pricing":[]});
    d.req("mark", "PUT", &format!("/api/admin/services/{id}/versions/{v}"), def.clone()).await.unwrap();
    let rejected = d
        .expect("mark", "POST", &format!("/api/admin/services/{id}/versions/{v}/publish"), json!({}), 422)
        .await
        .unwrap();
    assert!(rejected.to_string().contains("project_ref"), "{rejected}");
    def["fields"]
        .as_array_mut()
        .unwrap()
        .push(json!({"key":"which_project","type":"project_ref","label":"Building project","required":true}));
    d.req("mark", "PUT", &format!("/api/admin/services/{id}/versions/{v}"), def).await.unwrap();
    d.req("mark", "POST", &format!("/api/admin/services/{id}/versions/{v}/publish"), json!({})).await.unwrap();
    let (c, _) = d
        .submit("ben", "retaining-wall-notice", json!({"which_project":project.to_string()}), Some(org))
        .await
        .unwrap();
    assert_eq!(scalar(&d, &format!("SELECT building_project_id FROM cases WHERE id={c}")).await, project);
    assert_eq!(
        scalar(&d, &format!("SELECT COUNT(*) FROM case_links WHERE from_case_id={c} AND kind='follow_up_of'")).await,
        1
    );
    let history = d.req("ben", "GET", &format!("/api/building-projects/{project}"), json!({})).await.unwrap();
    assert!(history["cases"].as_array().unwrap().iter().any(|x| x["id"] == c));
}

#[tokio::test]
async fn catalogue_upgrade_versions_seeded_services_and_keeps_staff_edits_and_old_cases() {
    let (mut d, _dir) = support::fixture().await;
    // An installation from before this release: the six new services are absent and every seeded version
    // carries the `legacy` provenance written by migration 0803.
    let new = "('builder-stage-a-notice','builder-stage-b-notice','builder-stage-c-notice','builder-stage-d-notice','builder-stage-e-notice','pipeline-conduit-crossing')";
    for sql in [
        format!("DELETE FROM service_search WHERE service_id IN (SELECT id FROM services WHERE slug IN {new})"),
        format!("DELETE FROM service_versions WHERE service_id IN (SELECT id FROM services WHERE slug IN {new})"),
        format!("DELETE FROM services WHERE slug IN {new}"),
        "UPDATE service_versions SET seed_hash='legacy'".into(),
    ] {
        sqlx::query(&sql).execute(&d.state.db).await.unwrap();
    }
    // The commencement notice is still on its pre-audit definition: no building role, typed project reference.
    let (service, current): (i64, String) = sqlx::query_as("SELECT s.id,v.definition_json FROM services s JOIN service_versions v ON v.service_id=s.id AND v.status='published' WHERE s.slug='building-commencement-notice'").fetch_one(&d.state.db).await.unwrap();
    let mut old: Value = serde_json::from_str(&current).unwrap();
    old.as_object_mut().unwrap().remove("building_role");
    old["fields"].as_array_mut().unwrap().iter_mut().filter(|f| f["key"] == "project_reference").for_each(|f| {
        f["type"] = json!("text");
        f.as_object_mut().unwrap().remove("help");
    });
    sqlx::query("UPDATE service_versions SET status='retired' WHERE service_id=? AND status='published'")
        .bind(service)
        .execute(&d.state.db)
        .await
        .unwrap();
    let old_version: i64 = sqlx::query_scalar("INSERT INTO service_versions(service_id,version,status,definition_json,source_note,created_at,published_at,seed_hash) VALUES(?,2,'published',?,'Fictional demonstration configuration based on NIRC form: old.','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z','legacy') RETURNING id")
        .bind(service).bind(old.to_string()).fetch_one(&d.state.db).await.unwrap();
    let org = ben_org(&d).await;
    let project = new_project(&mut d, org).await;
    let (open, _) = d
        .submit("ben", "building-commencement-notice", json!({"project_reference":project.to_string()}), Some(org))
        .await
        .unwrap();
    // Legacy definitions without building_role still link through their seeded provenance.
    assert_eq!(scalar(&d, &format!("SELECT building_project_id FROM cases WHERE id={open}")).await, project);
    let frozen = d.detail("ben", open).await.unwrap()["definition"].clone();
    // Staff edited the published planning certificate in the builder.
    let cert: i64 = scalar(&d, "SELECT id FROM services WHERE slug='planning-certificate'").await;
    let draft = d.req("mark", "POST", &format!("/api/admin/services/{cert}/versions"), json!({})).await.unwrap()["id"]
        .as_i64()
        .unwrap();
    let mut edited: Value = serde_json::from_str(
        &sqlx::query_scalar::<_, String>("SELECT definition_json FROM service_versions WHERE id=?")
            .bind(draft)
            .fetch_one(&d.state.db)
            .await
            .unwrap(),
    )
    .unwrap();
    edited["summary"] = json!("Staff wording for the planning certificate.");
    d.req("mark", "PUT", &format!("/api/admin/services/{cert}/versions/{draft}"), edited.clone()).await.unwrap();
    d.req("mark", "POST", &format!("/api/admin/services/{cert}/versions/{draft}/publish"), json!({})).await.unwrap();

    let changes = upgrade(&d).await;
    for slug in ["builder-stage-a-notice", "builder-stage-e-notice", "pipeline-conduit-crossing"] {
        assert_eq!(action(&changes, slug).action, CatalogueAction::Created);
    }
    let commencement = action(&changes, "building-commencement-notice");
    assert_eq!((commencement.action, commencement.previous), (CatalogueAction::Upgraded, Some(2)));
    assert_eq!(action(&changes, "planning-certificate").action, CatalogueAction::DraftForReview);
    assert_eq!(action(&changes, "rawson-hall-hire").action, CatalogueAction::Unchanged);
    assert_eq!(action(&changes, "dog-registration").action, CatalogueAction::Unchanged);
    // Seeded, unchanged versions now carry the hash of their content.
    assert_eq!(scalar(&d, "SELECT COUNT(*) FROM service_versions WHERE seed_hash='legacy' AND status='published' AND service_id=(SELECT id FROM services WHERE slug='rawson-hall-hire')").await, 0);
    // The old case keeps its version and frozen definition; a new case uses the upgraded version.
    let new_version: i64 =
        scalar(&d, &format!("SELECT id FROM service_versions WHERE service_id={service} AND status='published'")).await;
    assert_ne!(new_version, old_version);
    assert_eq!(scalar(&d, &format!("SELECT status='retired' FROM service_versions WHERE id={old_version}")).await, 1);
    assert_eq!(scalar(&d, &format!("SELECT service_version_id FROM cases WHERE id={open}")).await, old_version);
    let after = d.detail("ben", open).await.unwrap();
    assert_eq!(after["definition"], frozen);
    assert_eq!(
        after["definition"]["fields"].as_array().unwrap().iter().find(|f| f["key"] == "project_reference").unwrap()["type"],
        "text"
    );
    d.action("olga", open, "advance").await.unwrap();
    assert_eq!(d.detail("olga", open).await.unwrap()["case"]["current_step"], "site");
    let (fresh, _) = d
        .submit("ben", "building-commencement-notice", json!({"project_reference":project.to_string()}), Some(org))
        .await
        .unwrap();
    assert_eq!(scalar(&d, &format!("SELECT service_version_id FROM cases WHERE id={fresh}")).await, new_version);
    assert_eq!(d.detail("ben", fresh).await.unwrap()["definition"]["building_role"], "follow_up");
    // The staff-published certificate stays published; the seed definition waits as a draft.
    let published: String = sqlx::query_scalar(&format!(
        "SELECT definition_json FROM service_versions WHERE service_id={cert} AND status='published'"
    ))
    .fetch_one(&d.state.db)
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&published).unwrap()["summary"],
        "Staff wording for the planning certificate."
    );
    assert_eq!(scalar(&d, &format!("SELECT COUNT(*) FROM service_versions WHERE service_id={cert} AND status='draft' AND seed_hash IS NOT NULL")).await, 1);

    // Idempotent: nothing more is written on the next runs, including the normal startup path.
    let versions = scalar(&d, "SELECT COUNT(*) FROM service_versions").await;
    let again = upgrade(&d).await;
    assert!(
        again.iter().all(|c| matches!(c.action, CatalogueAction::Unchanged | CatalogueAction::AwaitingReview)),
        "{again:?}"
    );
    assert_eq!(action(&again, "planning-certificate").action, CatalogueAction::AwaitingReview);
    servicehub::bootstrap::upgrade_catalogue(&d.state).await.unwrap();
    assert_eq!(scalar(&d, "SELECT COUNT(*) FROM service_versions").await, versions);
}
