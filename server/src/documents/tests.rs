use super::*;
use crate::{
    auth::{Actor, RoleGrant, UserKind},
    authz::{CaseAccess, Role},
    cases::core::{NewCase, Visibility},
    error::ErrorCode,
};
async fn fixture() -> (AppState, tempfile::TempDir, i64, Actor, Actor, Actor) {
    let (state, dir) = crate::state::test_support::test_state().await;
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    for (id, kind) in [(1, "resident"), (2, "staff"), (3, "staff"), (4, "resident")] {
        sqlx::query(
            "INSERT INTO users(id,email,display_name,kind,password_hash,created_at) VALUES(?,?,?,?,'unused',?)",
        )
        .bind(id)
        .bind(format!("fictional-{id}@example.test"))
        .bind(format!("Fictional {id}"))
        .bind(kind)
        .bind(crate::time::now_str())
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    sqlx::query("INSERT INTO role_grants(user_id,role,granted_at) VALUES(2,'specialist','2026-10-07')")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO services(id,slug,name,category,module,department,created_at) VALUES(1,'development-application','Development application','Planning','building','Planning',?)").bind(crate::time::now_str()).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO service_versions(id,service_id,version,status,definition_json,created_at) VALUES(1,1,1,'published','{}',?)").bind(crate::time::now_str()).execute(&mut *tx).await.unwrap();
    let c = crate::cases::core::create_case(
        &mut tx,
        NewCase {
            service_id: 1,
            service_version_id: 1,
            module: "building".into(),
            title: "Fictional dwelling".into(),
            status: "submitted".into(),
            applicant_user_id: Some(1),
            applicant_org_id: None,
            applicant_name: "Fictional owner".into(),
            applicant_email: None,
            applicant_phone: None,
            intake_channel: "online".into(),
            recorded_by_user_id: None,
            property_ref: Some("Portion DEMO-44".into()),
        },
    )
    .await
    .unwrap();
    let actor = |id, kind, roles: Vec<Role>| Actor {
        user_id: id,
        kind,
        roles: roles.into_iter().map(|role| RoleGrant { role, scope_service_id: None }).collect(),
        display_name: format!("Fictional {id}"),
        mfa_passed: true,
    };
    let resident = actor(1, UserKind::Resident, vec![]);
    let specialist = actor(2, UserKind::Staff, vec![Role::Specialist]);
    let admin = actor(3, UserKind::Staff, vec![Role::Sysadmin]);
    tx.commit().await.unwrap();
    (state, dir, c.id, resident, specialist, admin)
}
#[tokio::test]
async fn immutable_versions_visibility_revocation_and_exact_evidence() {
    let (state, _dir, cid, resident, specialist, _) = fixture().await;
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    let (doc, v1) = api::attach_generated(
        &mut tx,
        &state,
        cid,
        "plans",
        "Site plan",
        Visibility::Applicant,
        crate::pdf::simple_document("v1", &[], &[]),
        Some(1),
    )
    .await
    .unwrap();
    let (_, internal) = api::attach_generated(
        &mut tx,
        &state,
        cid,
        "internal",
        "Internal advice",
        Visibility::Staff,
        crate::pdf::simple_document("internal", &[], &[]),
        Some(2),
    )
    .await
    .unwrap();
    let blob: i64 = sqlx::query_scalar("SELECT blob_id FROM document_versions WHERE id=?")
        .bind(v1)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let v2: i64 = sqlx::query_scalar(
        "INSERT INTO document_versions(document_id,version,blob_id,uploaded_at) VALUES(?,2,?,?) RETURNING id",
    )
    .bind(doc)
    .bind(blob)
    .bind(crate::time::now_str())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(
        sqlx::query("UPDATE document_versions SET note='overwrite' WHERE id=?")
            .bind(v1)
            .execute(&mut *tx)
            .await
            .is_err()
    );
    for visibility in ["internal", "applicant"] {
        sqlx::query("INSERT INTO document_comments(document_version_id,author_user_id,visibility,body,created_at) VALUES(?,2,?,'Comment',?)").bind(v1).bind(visibility).bind(crate::time::now_str()).execute(&mut *tx).await.unwrap();
    }
    let view = uploads::project(&mut tx, cid, CaseAccess::Applicant).await.unwrap();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].versions.len(), 2);
    assert_eq!(view[0].versions[0].comments.len(), 1);
    assert_eq!(uploads::version_access(&mut tx, &resident, internal).await.err().unwrap().code, ErrorCode::NotFound);
    assert!(uploads::version_access(&mut tx, &specialist, internal).await.is_ok());
    let mut representative = resident.clone();
    representative.user_id = 4;
    sqlx::query("INSERT INTO case_representatives(case_id,user_id,basis,status,created_at) VALUES(?,4,'Fictional authorisation','active',?)").bind(cid).bind(crate::time::now_str()).execute(&mut *tx).await.unwrap();
    assert!(uploads::version_access(&mut tx, &representative, v1).await.is_ok());
    sqlx::query("UPDATE case_representatives SET status='revoked' WHERE case_id=? AND user_id=4")
        .bind(cid)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(uploads::version_access(&mut tx, &representative, v1).await.err().unwrap().code, ErrorCode::NotFound);
    let did:i64=sqlx::query_scalar("INSERT INTO decisions(case_id,decision_type,outcome,reasons,status,prepared_by,created_at) VALUES(?,'development_approval','approved','Reasons','draft',2,?) RETURNING id").bind(cid).bind(crate::time::now_str()).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO decision_evidence(decision_id,document_version_id) VALUES(?,?)")
        .bind(did)
        .bind(v1)
        .execute(&mut *tx)
        .await
        .unwrap();
    let evidence = decisions::evidence(&mut tx, did).await.unwrap();
    assert_eq!(evidence[0].id, v1);
    assert_ne!(evidence[0].id, v2);
    sqlx::query("UPDATE decisions SET status='issued',issued_at=? WHERE id=?")
        .bind(crate::time::now_str())
        .bind(did)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        sqlx::query("DELETE FROM decision_evidence WHERE decision_id=?").bind(did).execute(&mut *tx).await.is_err()
    );
    assert!(
        sqlx::query("UPDATE decisions SET reasons='changed' WHERE id=?").bind(did).execute(&mut *tx).await.is_err()
    );
}
#[tokio::test]
async fn authority_is_explicit_scoped_and_revocable_and_seeds_idempotent() {
    let (state, _dir, cid, _, specialist, admin) = fixture().await;
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    let case = crate::cases::core::load_case(&mut tx, cid).await.unwrap();
    assert!(!decisions::authority(&mut tx, &admin, &case, "development_approval").await.unwrap());
    assert!(!decisions::authority(&mut tx, &specialist, &case, "development_approval").await.unwrap());
    sqlx::query("INSERT INTO decision_authorities(user_id,decision_type,service_id,granted_by,granted_at) VALUES(2,'development_approval',1,3,?)").bind(crate::time::now_str()).execute(&mut *tx).await.unwrap();
    assert!(decisions::authority(&mut tx, &specialist, &case, "development_approval").await.unwrap());
    assert!(!decisions::authority(&mut tx, &specialist, &case, "building_approval").await.unwrap());
    sqlx::query("UPDATE decision_authorities SET revoked_at=?")
        .bind(crate::time::now_str())
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(!decisions::authority(&mut tx, &specialist, &case, "development_approval").await.unwrap());
    seed(&mut tx, &state).await.unwrap();
    seed(&mut tx, &state).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM decision_templates").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(n, 7);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM documents_seed_files").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(n, 2);
}
#[test]
fn placeholder_allowlist_and_no_recursive_expansion() {
    assert!(decisions::validate_template("{{unknown}}").is_err());
    assert!(decisions::validate_template("{{case_number").is_err());
    assert_eq!(
        decisions::render_template(
            "{{applicant_name}}",
            &[("applicant_name", "{{reasons}}".into()), ("reasons", "secret".into())]
        )
        .unwrap(),
        "{{reasons}}"
    );
    assert!(exhibition::validate_rectangles(&[exhibition::Rect { page: 1, x: 0.9, y: 0.0, w: 0.2, h: 0.1 }]).is_err());
    assert!(
        exhibition::validate_rectangles(&[exhibition::Rect { page: 1, x: f64::NAN, y: 0.0, w: 0.1, h: 0.1 }]).is_err()
    );
}
#[tokio::test]
async fn redaction_has_no_text_layer_and_source_remains_unchanged() {
    if std::process::Command::new("pdftoppm").arg("-v").output().is_err()
        || std::process::Command::new("pdftotext").arg("-v").output().is_err()
    {
        panic!("Poppler is required for the redaction regression suite.");
    }
    let source = crate::pdf::simple_document(
        "Fictional elevation",
        &[],
        &[("Owner phone", "CONFIDENTIAL-OWNER-PHONE 0412 345 678".into())],
    );
    let (published, _) = exhibition::redacted(
        &source,
        "application/pdf",
        &[exhibition::Rect { page: 1, x: 0.0, y: 0.1, w: 1.0, h: 0.6 }],
    )
    .await
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("source.pdf");
    let dst = dir.path().join("published.pdf");
    std::fs::write(&src, &source).unwrap();
    std::fs::write(&dst, &published).unwrap();
    let text = |path: &std::path::Path| {
        String::from_utf8(std::process::Command::new("pdftotext").arg(path).arg("-").output().unwrap().stdout).unwrap()
    };
    assert!(text(&src).contains("CONFIDENTIAL-OWNER-PHONE 0412 345 678"));
    assert!(text(&dst).trim().is_empty());
    let doc = printpdf::lopdf::Document::load_mem(&published).unwrap();
    assert!(!doc.trailer.has(b"Info"));
    assert!(!doc.objects.values().any(|o| o.as_dict().is_ok_and(|d| d.has(b"Metadata") || d.has(b"EmbeddedFiles"))));
}
#[tokio::test]
async fn automatic_closure_blocks_guard_until_window_ends() {
    let (state, _dir, cid, _, _, _) = fixture().await;
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    let case = crate::cases::core::load_case(&mut tx, cid).await.unwrap();
    assert!(hooks::step_guard_handler(&mut tx, &case, "documents.exhibition_closed").await.unwrap().is_some());
    sqlx::query("INSERT INTO exhibitions(case_id,title,summary,status,opens_at,closes_at,prepared_by,created_at) VALUES(?,'Fictional notice','Summary','open','2020-01-01T00:00:00Z','2020-01-02T00:00:00Z',2,?)").bind(cid).bind(crate::time::now_str()).execute(&mut *tx).await.unwrap();
    assert!(hooks::step_guard_handler(&mut tx, &case, "documents.exhibition_closed").await.unwrap().is_none());
}

#[tokio::test]
async fn issue_endpoint_rejects_sysadmin_even_with_case_role_and_preparer_cannot_publish() {
    use crate::web::{Json, Path};
    use axum::extract::State;
    let (state, _dir, cid, _, specialist, mut admin) = fixture().await;
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    let did:i64=sqlx::query_scalar("INSERT INTO decisions(case_id,decision_type,outcome,reasons,status,prepared_by,created_at) VALUES(?,'development_approval','approved','Fictional reasons','pending_approval',2,?) RETURNING id").bind(cid).bind(crate::time::now_str()).fetch_one(&mut *tx).await.unwrap();
    let eid:i64=sqlx::query_scalar("INSERT INTO exhibitions(case_id,title,summary,status,opens_at,closes_at,prepared_by,created_at) VALUES(?,'Fictional notice','Summary','draft','2026-01-01T00:00:00Z','2099-01-01T00:00:00Z',2,?) RETURNING id").bind(cid).bind(crate::time::now_str()).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    admin.roles.push(RoleGrant { role: Role::Specialist, scope_service_id: None });
    let error = decisions::action(
        State(state.clone()),
        admin,
        Path((cid, did, "issue".into())),
        Json(decisions::ActionInput { expected_revision: 1, reason: None }),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.code, ErrorCode::Forbidden);
    let input: exhibition::Revision = serde_json::from_value(serde_json::json!({"expected_revision":1})).unwrap();
    let error = exhibition::publish(State(state), specialist, Path(eid), Json(input)).await.err().unwrap();
    assert_eq!(error.code, ErrorCode::Forbidden);
}

#[tokio::test]
async fn project_creation_modification_links_and_supersession_reference_are_stable() {
    let (state, _dir, cid, resident, _, _) = fixture().await;
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    let case = crate::cases::core::load_case(&mut tx, cid).await.unwrap();
    building::on_submit(&mut tx, &state, &resident, &case, &serde_json::json!({"property_ref":"Portion DEMO-44"}))
        .await
        .unwrap();
    let original = crate::cases::core::load_case(&mut tx, cid).await.unwrap();
    building::on_submit(&mut tx, &state, &resident, &original, &serde_json::json!({"property_ref":"Portion DEMO-44"}))
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM building_projects").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(n, 1);
    let did:i64=sqlx::query_scalar("INSERT INTO decisions(case_id,decision_type,outcome,reasons,status,prepared_by,created_at,issued_at) VALUES(?,'development_approval','approved','Fictional reasons','issued',2,?,?) RETURNING id").bind(cid).bind(crate::time::now_str()).bind(crate::time::now_str()).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO services(id,slug,name,category,module,department,created_at) VALUES(2,'modify-approval','Modify approval','Planning','building','Planning',?)").bind(crate::time::now_str()).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO service_versions(id,service_id,version,status,definition_json,created_at) VALUES(2,2,1,'published','{}',?)").bind(crate::time::now_str()).execute(&mut *tx).await.unwrap();
    let modified = crate::cases::core::create_case(
        &mut tx,
        NewCase {
            service_id: 2,
            service_version_id: 2,
            module: "building".into(),
            title: "Fictional modification".into(),
            status: "submitted".into(),
            applicant_user_id: Some(1),
            applicant_org_id: None,
            applicant_name: "Fictional owner".into(),
            applicant_email: None,
            applicant_phone: None,
            intake_channel: "online".into(),
            recorded_by_user_id: None,
            property_ref: Some("Portion DEMO-44".into()),
        },
    )
    .await
    .unwrap();
    building::on_submit(
        &mut tx,
        &state,
        &resident,
        &modified,
        &serde_json::json!({"original_approval":{"decision_id":did}}),
    )
    .await
    .unwrap();
    assert_eq!(
        crate::cases::core::load_case(&mut tx, modified.id).await.unwrap().building_project_id,
        original.building_project_id
    );
    let link: (i64, String) = sqlx::query_as("SELECT to_case_id,kind FROM case_links WHERE from_case_id=?")
        .bind(modified.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(link, (cid, "modification_of".into()));
    let linked: i64 = sqlx::query_scalar("SELECT decision_id FROM building_original_approvals WHERE case_id=?")
        .bind(modified.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(linked, did);
    let mut other = resident;
    other.user_id = 4;
    assert!(
        building::on_submit(
            &mut tx,
            &state,
            &other,
            &modified,
            &serde_json::json!({"original_approval":{"decision_id":did}})
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn public_submission_window_and_public_projection_never_expose_source() {
    use crate::web::{ClientIp, Json, Path};
    use axum::extract::State;
    let (state, _dir, cid, _, _, _) = fixture().await;
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    let eid:i64=sqlx::query_scalar("INSERT INTO exhibitions(case_id,title,summary,status,opens_at,closes_at,prepared_by,created_at) VALUES(?,'Fictional notice','Summary','open',?,?,2,?) RETURNING id").bind(cid).bind((state.now()-chrono::Duration::days(1)).to_rfc3339()).bind((state.now()+chrono::Duration::days(1)).to_rfc3339()).bind(crate::time::now_str()).fetch_one(&mut *tx).await.unwrap();
    let (_, vid) = api::attach_generated(
        &mut tx,
        &state,
        cid,
        "plans",
        "Source title",
        Visibility::Applicant,
        crate::pdf::simple_document("Private source", &[], &[]),
        Some(1),
    )
    .await
    .unwrap();
    let item:i64=sqlx::query_scalar("INSERT INTO exhibition_items(exhibition_id,source_document_version_id,title,created_at) VALUES(?,?,'Public item',?) RETURNING id").bind(eid).bind(vid).bind(crate::time::now_str()).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let Json(view) = exhibition::public_detail(State(state.clone()), Path(eid)).await.unwrap();
    assert!(!view.to_string().contains("source_document_version_id"));
    assert!(!view.to_string().contains("prepared_by"));
    assert_eq!(view["items"].as_array().unwrap().len(), 0);
    let error = exhibition::public_file(State(state.clone()), Path((eid, item))).await.err().unwrap();
    assert_eq!(error.code, ErrorCode::NotFound);
    let submission:exhibition::Submission=serde_json::from_value(serde_json::json!({"name":"Fictional neighbour","email":"neighbour@example.test","body":"Please consider the fictional access road."})).unwrap();
    assert!(
        exhibition::submit(State(state.clone()), ClientIp("test-1".into()), Path(eid), Json(submission)).await.is_ok()
    );
    sqlx::query("UPDATE exhibitions SET closes_at=? WHERE id=?")
        .bind((state.now() - chrono::Duration::hours(1)).to_rfc3339())
        .bind(eid)
        .execute(&state.db)
        .await
        .unwrap();
    let submission: exhibition::Submission = serde_json::from_value(
        serde_json::json!({"name":"Fictional neighbour","email":"neighbour@example.test","body":"Too late."}),
    )
    .unwrap();
    let error =
        exhibition::submit(State(state), ClientIp("test-2".into()), Path(eid), Json(submission)).await.err().unwrap();
    assert_eq!(error.code, ErrorCode::Conflict);
}

async fn multipart(state: &AppState, file: &[u8], fields: &[(&str, String)]) -> uploads::DocsMultipart {
    use axum::{body::Body, extract::FromRequest, http::Request};
    let boundary = "S2TestUploadBoundary";
    let mut body = Vec::new();
    for (key, value) in fields {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n").as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"drawing.pdf\"\r\nContent-Type: application/pdf\r\n\r\n").as_bytes());
    body.extend_from_slice(file);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    uploads::DocsMultipart::from_request(
        Request::builder()
            .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
            .body(Body::from(body))
            .unwrap(),
        state,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn replacement_resolves_only_earlier_comments_on_same_document_and_preserves_source() {
    use crate::web::Path;
    use axum::extract::State;
    let (state, _dir, cid, resident, _, _) = fixture().await;
    let first = crate::pdf::simple_document("Fictional original plan", &[], &[]);
    let form = multipart(&state, &first, &[("title", "Drawing A-101".into())]).await;
    let crate::web::Json(doc) = uploads::upload(State(state.clone()), resident.clone(), Path(cid), form).await.unwrap();
    let did = doc["id"].as_i64().unwrap();
    let v1 = doc["version_id"].as_i64().unwrap();
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    let comment: i64 = sqlx::query_scalar("INSERT INTO document_comments(document_version_id,author_user_id,visibility,body,created_at,request_new_version) VALUES(?,2,'applicant','Replace the setback dimension',?,1) RETURNING id")
        .bind(v1).bind(crate::time::now_str()).fetch_one(&mut *tx).await.unwrap();
    let (_, unrelated_version) = api::attach_generated(
        &mut tx,
        &state,
        cid,
        "plans",
        "Unrelated elevation",
        Visibility::Applicant,
        first.clone(),
        Some(1),
    )
    .await
    .unwrap();
    let unrelated: i64 = sqlx::query_scalar("INSERT INTO document_comments(document_version_id,author_user_id,visibility,body,created_at) VALUES(?,2,'applicant','Other drawing',?) RETURNING id")
        .bind(unrelated_version).bind(crate::time::now_str()).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let replacement = crate::pdf::simple_document("Fictional revised plan", &[], &[]);
    let bad = multipart(&state, &replacement, &[("resolves_comment_ids[]", unrelated.to_string())]).await;
    let error = uploads::version(State(state.clone()), resident.clone(), Path(did), bad).await.err().unwrap();
    assert_eq!(error.code, ErrorCode::Validation);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM document_versions WHERE document_id=?")
        .bind(did)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(n, 1, "invalid resolution must roll back the new version");
    let good = multipart(
        &state,
        &replacement,
        &[("note", "Setback dimension corrected".into()), ("resolves_comment_ids[]", comment.to_string())],
    )
    .await;
    let crate::web::Json(version) =
        uploads::version(State(state.clone()), resident.clone(), Path(did), good).await.unwrap();
    let v2 = version["version_id"].as_i64().unwrap();
    let resolved: i64 = sqlx::query_scalar("SELECT resolved_by_version_id FROM document_comments WHERE id=?")
        .bind(comment)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(resolved, v2);
    let blob: i64 = sqlx::query_scalar("SELECT blob_id FROM document_versions WHERE id=?")
        .bind(v1)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(crate::storage::read(&state, blob).await.unwrap().1, first);
    let notifications: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE user_id=2 AND case_id=?")
        .bind(cid)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(notifications, 1);
    let response = uploads::download(State(state.clone()), resident.clone(), Path(v1)).await.unwrap();
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert!(response.headers()["content-disposition"].to_str().unwrap().starts_with("attachment;"));
    let mut stranger = resident;
    stranger.user_id = 4;
    assert_eq!(uploads::download(State(state), stranger, Path(v1)).await.err().unwrap().code, ErrorCode::NotFound);
}

#[tokio::test]
async fn issued_pdf_pins_evidence_refusals_and_separate_approvals() {
    use crate::web::{Json, Path};
    use axum::extract::State;
    let (state, _dir, cid, _, specialist, _) = fixture().await;
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    seed(&mut tx, &state).await.unwrap();
    let (doc, v1) = api::attach_generated(
        &mut tx,
        &state,
        cid,
        "plans",
        "Site plan",
        Visibility::Applicant,
        crate::pdf::simple_document("Fictional plan", &[], &[]),
        Some(1),
    )
    .await
    .unwrap();
    let blob: i64 = sqlx::query_scalar("SELECT blob_id FROM document_versions WHERE id=?")
        .bind(v1)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let v2: i64 = sqlx::query_scalar(
        "INSERT INTO document_versions(document_id,version,blob_id,uploaded_at) VALUES(?,2,?,?) RETURNING id",
    )
    .bind(doc)
    .bind(blob)
    .bind(crate::time::now_str())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO decision_authorities(user_id,decision_type,granted_by,granted_at) VALUES(2,'development_approval',3,?),(2,'building_approval',3,?)").bind(crate::time::now_str()).bind(crate::time::now_str()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    for (kind, outcome) in [("development_approval", "approved_with_conditions"), ("building_approval", "refused")] {
        let revision =
            sqlx::query_scalar("SELECT revision FROM cases WHERE id=?").bind(cid).fetch_one(&state.db).await.unwrap();
        let template = sqlx::query_scalar("SELECT id FROM decision_templates WHERE decision_type=?")
            .bind(kind)
            .fetch_one(&state.db)
            .await
            .unwrap();
        let Json(created) = decisions::create(
            State(state.clone()),
            specialist.clone(),
            Path(cid),
            Json(decisions::Input {
                decision_type: kind.into(),
                outcome: outcome.into(),
                reasons: "Fictional assessment reasons".into(),
                conditions: "Retain the fictional access path".into(),
                template_id: template,
                evidence_version_ids: None,
                expected_revision: revision,
                supersedes_decision_id: None,
            }),
        )
        .await
        .unwrap();
        let id = created["id"].as_i64().unwrap();
        let mut tx = crate::db::write_tx(&state.db).await.unwrap();
        assert_eq!(decisions::evidence(&mut tx, id).await.unwrap()[0].id, v2);
        sqlx::query("UPDATE decisions SET status='pending_approval' WHERE id=?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap();
        let case = crate::cases::core::load_case(&mut tx, cid).await.unwrap();
        let decision = decisions::load(&mut tx, id, cid).await.unwrap();
        let approver = independent_approver(&mut tx).await;
        decisions::issue_document(&mut tx, &state, &approver, &case, decision).await.unwrap();
        let issued = decisions::load(&mut tx, id, cid).await.unwrap();
        assert_eq!(issued.status, "issued");
        assert_eq!(issued.outcome, outcome);
        assert!(issued.output_document_version_id.is_some());
        assert!(
            sqlx::query("UPDATE decision_evidence SET document_version_id=? WHERE decision_id=?")
                .bind(v1)
                .bind(id)
                .execute(&mut *tx)
                .await
                .is_err()
        );
        tx.commit().await.unwrap();
    }
    let mut conn = state.db.acquire().await.unwrap();
    assert_eq!(api::issued_decisions(&mut conn, cid).await.unwrap().len(), 2);
}

#[tokio::test]
async fn task_only_and_finance_document_permissions_follow_authz() {
    let (state, _dir, cid, _, mut staff, _) = fixture().await;
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    let (_, vid) = api::attach_generated(
        &mut tx,
        &state,
        cid,
        "plans",
        "Private plan",
        Visibility::Applicant,
        crate::pdf::simple_document("Fictional", &[], &[]),
        Some(1),
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO tasks(case_id,kind,title,instructions,assigned_to,status,created_at) VALUES(?,'general','Fictional inspection','Inspect the site',2,'open',?)").bind(cid).bind(crate::time::now_str()).execute(&mut *tx).await.unwrap();
    staff.roles = vec![RoleGrant { role: Role::FieldWorker, scope_service_id: None }];
    assert_eq!(uploads::version_access(&mut tx, &staff, vid).await.err().unwrap().code, ErrorCode::NotFound);
    staff.roles = vec![RoleGrant { role: Role::Finance, scope_service_id: None }];
    let (_, projection, _, _, _) = uploads::version_access(&mut tx, &staff, vid).await.unwrap();
    assert_eq!(uploads::writable(projection).unwrap_err().code, ErrorCode::Forbidden);
}

#[tokio::test]
async fn publishing_uses_new_blob_with_burned_pixels_and_independent_staff_approval() {
    use crate::web::{Json, Path};
    use axum::extract::State;
    if std::process::Command::new("pdftoppm").arg("-v").output().is_err()
        || std::process::Command::new("pdftotext").arg("-v").output().is_err()
    {
        panic!("Poppler is required for the redaction regression suite.");
    }
    let (state, dir, cid, _, specialist, mut manager) = fixture().await;
    manager.roles = vec![RoleGrant { role: Role::Manager, scope_service_id: None }];
    let source = include_bytes!("../../seed-data/docs/elevation.pdf");
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    let (_, vid) = api::attach_generated(
        &mut tx,
        &state,
        cid,
        "plans",
        "Fictional elevation A-201",
        Visibility::Applicant,
        source.to_vec(),
        Some(1),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let revision: i64 =
        sqlx::query_scalar("SELECT revision FROM cases WHERE id=?").bind(cid).fetch_one(&state.db).await.unwrap();
    let Json(created) = exhibition::create(
        State(state.clone()),
        specialist.clone(),
        Json(exhibition::Input {
            case_id: cid,
            title: "Fictional dwelling proposal".into(),
            summary: "Demo public exhibition".into(),
            opens_at: (state.now() - chrono::Duration::days(1)).to_rfc3339(),
            closes_at: (state.now() + chrono::Duration::days(15)).to_rfc3339(),
            expected_revision: revision,
        }),
    )
    .await
    .unwrap();
    let eid = created["id"].as_i64().unwrap();
    let Json(detail) = exhibition::detail(State(state.clone()), specialist.clone(), Path(eid)).await.unwrap();
    let Json(added) = exhibition::add_item(
        State(state.clone()),
        specialist.clone(),
        Path(eid),
        Json(exhibition::ItemInput {
            source_document_version_id: vid,
            title: "Public elevation A-201".into(),
            redactions: vec![exhibition::Rect { page: 1, x: 0.0, y: 0.1, w: 1.0, h: 0.6 }],
            expected_revision: detail["revision"].as_i64().unwrap(),
        }),
    )
    .await
    .unwrap();
    let item = added["id"].as_i64().unwrap();
    let Json(detail) = exhibition::detail(State(state.clone()), specialist, Path(eid)).await.unwrap();
    let expected_revision = detail["revision"].as_i64().unwrap();
    exhibition::publish(State(state.clone()), manager, Path(eid), Json(exhibition::Revision { expected_revision }))
        .await
        .unwrap();
    let (source_blob, published_blob):(i64,i64)=sqlx::query_as("SELECT v.blob_id,i.published_blob_id FROM exhibition_items i JOIN document_versions v ON v.id=i.source_document_version_id WHERE i.id=?").bind(item).fetch_one(&state.db).await.unwrap();
    assert_ne!(source_blob, published_blob);
    assert_eq!(crate::storage::read(&state, source_blob).await.unwrap().1, source);
    let published = crate::storage::read(&state, published_blob).await.unwrap().1;
    let path = dir.path().join("publication.pdf");
    std::fs::write(&path, &published).unwrap();
    let text = std::process::Command::new("pdftotext").arg(&path).arg("-").output().unwrap();
    assert!(text.status.success());
    assert!(String::from_utf8(text.stdout).unwrap().trim().is_empty());
    let render = std::process::Command::new("pdftoppm")
        .args(["-r", "110", "-png", "-singlefile"])
        .arg(&path)
        .arg(dir.path().join("published-page"))
        .output()
        .unwrap();
    assert!(render.status.success());
    let image = printpdf::image_crate::open(dir.path().join("published-page.png")).unwrap().to_rgb8();
    assert_eq!(image.get_pixel(image.width() / 2, image.height() / 3).0, [0, 0, 0], "redaction is burned into pixels");
    // The public preview must rasterise the published copy, never its private source.
    let response = exhibition::public_preview(State(state.clone()), Path((eid, item, "1.png".into()))).await.unwrap();
    assert_eq!(response.headers()["content-type"], "image/png");
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let preview = printpdf::image_crate::load_from_memory(&bytes).unwrap().to_rgb8();
    assert_eq!(preview.get_pixel(preview.width() / 2, preview.height() / 3).0, [0, 0, 0]);
    assert!(exhibition::public_preview(State(state.clone()), Path((eid, item, "0.png".into()))).await.is_err());
    assert!(exhibition::public_preview(State(state.clone()), Path((eid, item + 999, "1.png".into()))).await.is_err());
    let response = exhibition::public_file(State(state), Path((eid, item))).await.unwrap();
    assert_eq!(response.headers()["content-type"], "application/pdf");
}

#[tokio::test]
async fn planning_certificate_reissue_retains_old_result_and_requested_sections() {
    let (state, dir, cid, _, _specialist, _) = fixture().await;
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    seed(&mut tx, &state).await.unwrap();
    sqlx::query("UPDATE services SET module='planning_certificate',slug='planning-certificate' WHERE id=1")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE cases SET module='planning_certificate' WHERE id=?").bind(cid).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO submissions(case_id,answers_json,service_version_id,definition_snapshot_json,definition_sha256,submitted_by,submitted_at) VALUES(?,?,1,'{}','fictional-test-hash',1,?)",
    )
    .bind(cid)
    .bind(serde_json::json!({"property_ref":"Portion DEMO-44, Lot 2","sections":["Zoning","Heritage"]}).to_string())
    .bind(crate::time::now_str())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO decision_authorities(user_id,decision_type,granted_by,granted_at) VALUES(2,'planning_certificate',3,?)").bind(crate::time::now_str()).execute(&mut *tx).await.unwrap();
    let template: i64 =
        sqlx::query_scalar("SELECT id FROM decision_templates WHERE decision_type='planning_certificate'")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let case = crate::cases::core::load_case(&mut tx, cid).await.unwrap();
    let mut result_blobs = vec![];
    for information in ["Fictional information first issued", "Fictional information corrected"] {
        let id:i64=sqlx::query_scalar("INSERT INTO decisions(case_id,decision_type,outcome,reasons,status,template_id,prepared_by,created_at) VALUES(?,'planning_certificate','approved',?,'pending_approval',?,2,?) RETURNING id")
            .bind(cid).bind(information).bind(template).bind(crate::time::now_str()).fetch_one(&mut *tx).await.unwrap();
        let decision = decisions::load(&mut tx, id, cid).await.unwrap();
        let approver = independent_approver(&mut tx).await;
        decisions::issue_document(&mut tx, &state, &approver, &case, decision).await.unwrap();
        let blob:i64=sqlx::query_scalar("SELECT v.blob_id FROM decisions d JOIN document_versions v ON v.id=d.output_document_version_id WHERE d.id=?").bind(id).fetch_one(&mut *tx).await.unwrap();
        result_blobs.push(blob);
    }
    assert_eq!(api::issued_decisions(&mut tx, cid).await.unwrap().len(), 2);
    tx.commit().await.unwrap();
    assert_ne!(result_blobs[0], result_blobs[1]);
    if std::process::Command::new("pdftotext").arg("-v").output().is_err() {
        panic!("Poppler is required for the redaction regression suite.");
    }
    let path = dir.path().join("certificate.pdf");
    std::fs::write(&path, crate::storage::read(&state, result_blobs[0]).await.unwrap().1).unwrap();
    let text = std::process::Command::new("pdftotext").arg(path).arg("-").output().unwrap();
    let text = String::from_utf8(text.stdout).unwrap();
    for expected in [
        "Portion DEMO-44, Lot 2",
        "Zoning",
        "Heritage",
        "Information as recorded on",
        "Fictional information first issued",
    ] {
        assert!(text.contains(expected), "{expected} missing from certificate");
    }
}

#[tokio::test]
async fn image_redaction_is_opaque_and_has_no_alpha_channel() {
    use printpdf::image_crate as image;
    let source = image::RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 100]));
    let mut bytes = std::io::Cursor::new(vec![]);
    image::DynamicImage::ImageRgba8(source).write_to(&mut bytes, image::ImageOutputFormat::Png).unwrap();
    let (published, name) = exhibition::redacted(
        bytes.get_ref(),
        "image/png",
        &[exhibition::Rect { page: 1, x: 0.2, y: 0.2, w: 0.4, h: 0.4 }],
    )
    .await
    .unwrap();
    assert_eq!(name, "public-copy.png");
    let decoded = image::load_from_memory(&published).unwrap();
    assert!(!decoded.color().has_alpha());
    let decoded = decoded.to_rgb8();
    assert_eq!(decoded.get_pixel(5, 5).0, [0, 0, 0]);
    assert_ne!(decoded.get_pixel(0, 0).0, [0, 0, 0]);
    assert!(
        exhibition::redacted(
            bytes.get_ref(),
            "image/png",
            &[exhibition::Rect { page: 2, x: 0.2, y: 0.2, w: 0.4, h: 0.4 }]
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn rejected_uploads_register_nothing() {
    use crate::web::Path;
    use axum::extract::State;
    let (state, _dir, cid, resident, _, _) = fixture().await;
    let file = crate::pdf::simple_document("Rejected bytes", &[], &[]);
    for fields in [
        vec![("visibility", "staff".into())],
        vec![("requirement_key", "nonexistent".into())],
        vec![("expected_revision", "0".into())],
    ] {
        let form = multipart(&state, &file, &fields).await;
        assert!(uploads::upload(State(state.clone()), resident.clone(), Path(cid), form).await.is_err());
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM blobs").fetch_one(&state.db).await.unwrap(), 0);
    }
}

#[tokio::test]
async fn applicant_cannot_upload_generated_categories_or_version_generated_documents() {
    use crate::web::Path;
    use axum::extract::State;
    let (state, _dir, cid, resident, _, _) = fixture().await;
    let file = crate::pdf::simple_document("Applicant bytes", &[], &[]);
    for category in ["booking_confirmation", "invoice", "credit_note", "certificate", "made_up"] {
        let form = multipart(&state, &file, &[("category", category.into())]).await;
        assert!(uploads::upload(State(state.clone()), resident.clone(), Path(cid), form).await.is_err());
    }
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    let (invoice, _) = api::attach_generated(
        &mut tx,
        &state,
        cid,
        "invoice",
        "Issued invoice",
        Visibility::Applicant,
        file.clone(),
        Some(2),
    )
    .await
    .unwrap();
    let (provenance, _) = api::attach_generated(
        &mut tx,
        &state,
        cid,
        "supporting",
        "Generated attachment",
        Visibility::Applicant,
        file.clone(),
        Some(2),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    for doc in [invoice, provenance] {
        let form = multipart(&state, &file, &[]).await;
        let err = uploads::version(State(state.clone()), resident.clone(), Path(doc), form).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM document_versions").fetch_one(&state.db).await.unwrap(),
        2
    );
}

#[tokio::test]
async fn closed_case_cannot_issue_pending_decision_or_create_artifact() {
    use crate::web::{Json, Path};
    use axum::extract::State;
    let (state, _dir, cid, _, specialist, _) = fixture().await;
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    sqlx::query("INSERT INTO decision_authorities(user_id,decision_type,granted_by,granted_at) VALUES(2,'development_approval',3,'2026-10-07')").execute(&mut *tx).await.unwrap();
    seed(&mut tx, &state).await.unwrap();
    let template: i64 =
        sqlx::query_scalar("SELECT id FROM decision_templates WHERE decision_type='development_approval' LIMIT 1")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let did:i64=sqlx::query_scalar("INSERT INTO decisions(case_id,decision_type,outcome,reasons,status,prepared_by,template_id,created_at) VALUES(?,'development_approval','approved','Reasons','pending_approval',2,?,'2026-10-07') RETURNING id").bind(cid).bind(template).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("UPDATE cases SET status='withdrawn',closed_at='2026-10-07T00:00:00Z' WHERE id=?")
        .bind(cid)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let err = decisions::action(
        State(state.clone()),
        specialist,
        Path((cid, did, "issue".into())),
        Json(decisions::ActionInput { expected_revision: 1, reason: None }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM decisions WHERE id=?")
            .bind(did)
            .fetch_one(&state.db)
            .await
            .unwrap(),
        "pending_approval"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM documents WHERE case_id=?")
            .bind(cid)
            .fetch_one(&state.db)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn enormous_pdf_geometry_is_rendered_with_bounded_dimensions() {
    let (doc, _, _) = printpdf::PdfDocument::new("Huge", printpdf::Mm(700.0), printpdf::Mm(700.0), "Page");
    let source = doc.save_to_bytes().unwrap();
    let (bytes, _) = exhibition::redacted(&source, "application/pdf", &[]).await.unwrap();
    let pdf = printpdf::lopdf::Document::load_mem(&bytes).unwrap();
    for object in pdf.objects.values() {
        if let Ok(stream) = object.as_stream()
            && stream.dict.get(b"Subtype").and_then(|v| v.as_name()).ok() == Some(b"Image".as_slice())
        {
            assert!(stream.dict.get(b"Width").unwrap().as_i64().unwrap() <= 1800);
            assert!(stream.dict.get(b"Height").unwrap().as_i64().unwrap() <= 1800);
        }
    }
    assert!(bytes.len() < crate::storage::MAX_BYTES);
}

#[tokio::test]
async fn inactive_authority_does_not_rollback_decision_submission() {
    use crate::web::{Json, Path};
    use axum::extract::State;
    let (state, _dir, cid, _, specialist, _) = fixture().await;
    let mut tx = crate::db::write_tx(&state.db).await.unwrap();
    sqlx::query("UPDATE users SET is_active=0 WHERE id=3").execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO decision_authorities(user_id,decision_type,granted_by,granted_at) VALUES(3,'development_approval',2,'2026-10-07')").execute(&mut *tx).await.unwrap();
    let did:i64=sqlx::query_scalar("INSERT INTO decisions(case_id,decision_type,outcome,reasons,status,prepared_by,created_at) VALUES(?,'development_approval','approved','Reasons','draft',2,'2026-10-07') RETURNING id").bind(cid).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    decisions::action(
        State(state.clone()),
        specialist,
        Path((cid, did, "submit".into())),
        Json(decisions::ActionInput { expected_revision: 1, reason: None }),
    )
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM decisions WHERE id=?")
            .bind(did)
            .fetch_one(&state.db)
            .await
            .unwrap(),
        "pending_approval"
    );
}

#[tokio::test]
async fn aggregate_upload_quota_rejects_attachment_without_registering_more_bytes() {
    use crate::web::Path;
    use axum::extract::State;
    let (state, _dir, cid, resident, _, _) = fixture().await;
    let first = crate::pdf::simple_document("Existing bytes", &[], &[]);
    let form = multipart(&state, &first, &[]).await;
    uploads::upload(State(state.clone()), resident.clone(), Path(cid), form).await.unwrap();
    // Model an account with 100 MB of retained applicant uploads.
    sqlx::query("UPDATE blobs SET size_bytes=104857600").execute(&state.db).await.unwrap();
    let extra = crate::pdf::simple_document("Extra bytes", &[], &[]);
    let form = multipart(&state, &extra, &[]).await;
    let err = uploads::upload(State(state.clone()), resident, Path(cid), form).await.unwrap_err();
    assert!(err.fields["file"].contains("100 MB"));
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM blobs").fetch_one(&state.db).await.unwrap(), 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM document_versions").fetch_one(&state.db).await.unwrap(),
        1
    );
}

async fn independent_approver(tx: &mut sqlx::SqliteConnection) -> Actor {
    sqlx::query("INSERT INTO users(id,email,display_name,kind,created_at) VALUES(5,'approver@example.test','Independent approver','staff','2026-10-07') ON CONFLICT DO NOTHING").execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO role_grants(user_id,role,granted_at) SELECT 5,'manager','2026-10-07' WHERE NOT EXISTS(SELECT 1 FROM role_grants WHERE user_id=5 AND role='manager')").execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO decision_authorities(user_id,decision_type,granted_by,granted_at) SELECT 5,decision_type,2,'2026-10-07' FROM decision_authorities WHERE user_id=2 AND revoked_at IS NULL AND NOT EXISTS(SELECT 1 FROM decision_authorities a WHERE a.user_id=5 AND a.decision_type=decision_authorities.decision_type)").execute(&mut *tx).await.unwrap();
    Actor::load(tx, 5, true).await.unwrap()
}
