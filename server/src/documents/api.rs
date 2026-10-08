//! Transaction-composable documents API.
use crate::{
    auth::Actor,
    cases::core::{CaseRow, Visibility},
    error::{AppError, AppResult},
    services::definition::StepDef,
    state::AppState,
    storage, time,
};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
pub type DocumentId = i64;
pub type DocumentVersionId = i64;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct DecisionSummary {
    pub id: i64,
    pub decision_type: String,
    pub outcome: String,
    pub status: String,
    pub issued_at: Option<String>,
    pub output_document_version_id: Option<i64>,
}
#[allow(clippy::too_many_arguments)]
pub async fn attach_generated(
    tx: &mut SqliteConnection,
    state: &AppState,
    case_id: i64,
    category: &str,
    title: &str,
    visibility: Visibility,
    pdf_bytes: Vec<u8>,
    actor: Option<i64>,
) -> AppResult<(DocumentId, DocumentVersionId)> {
    let staged = storage::stage(state, &pdf_bytes, "document.pdf", storage::AllowList::Docs).await?;
    let blob = storage::register(tx, staged, actor).await?;
    let result = insert(tx, case_id, category, title, visibility, None, blob.id, actor).await?;
    sqlx::query("UPDATE documents SET generated=1 WHERE id=?").bind(result.0).execute(tx).await?;
    Ok(result)
}
#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert(
    tx: &mut SqliteConnection,
    case_id: i64,
    category: &str,
    title: &str,
    visibility: Visibility,
    requirement: Option<&str>,
    blob: i64,
    actor: Option<i64>,
) -> AppResult<(i64, i64)> {
    let id=sqlx::query_scalar("INSERT INTO documents(case_id,category,title,visibility,requirement_key,created_by,created_at) VALUES(?,?,?,?,?,?,?) RETURNING id").bind(case_id).bind(category).bind(title).bind(visibility.as_str()).bind(requirement).bind(actor).bind(time::now_str()).fetch_one(&mut *tx).await?;
    let vid=sqlx::query_scalar("INSERT INTO document_versions(document_id,version,blob_id,uploaded_by,uploaded_at) VALUES(?,1,?,?,?) RETURNING id").bind(id).bind(blob).bind(actor).bind(time::now_str()).fetch_one(&mut *tx).await?;
    Ok((id, vid))
}
pub async fn issued_decisions(tx: &mut SqliteConnection, case_id: i64) -> AppResult<Vec<DecisionSummary>> {
    Ok(sqlx::query_as("SELECT id,decision_type,outcome,status,issued_at,output_document_version_id FROM decisions WHERE case_id=? AND status='issued' ORDER BY id").bind(case_id).fetch_all(tx).await?)
}
/// Response letters a workflow `module` step can wait for (`documents.letter_issued:<type>`).
pub const LETTER_TYPES: &[&str] = &["service_response", "road_response", "complaint_response"];
/// Staff roles that may issue a response letter.
pub const LETTER_ROLES: &[crate::authz::Role] = &[
    crate::authz::Role::ComplaintsOfficer,
    crate::authz::Role::Intake,
    crate::authz::Role::Specialist,
    crate::authz::Role::Manager,
];
pub fn letter_label(letter_type: &str) -> &'static str {
    match letter_type {
        "road_response" => "road issue response",
        "complaint_response" => "complaint response",
        _ => "service response",
    }
}
/// `(step, letter type)` for every response-letter step of the case's frozen workflow.
pub async fn letter_steps(tx: &mut SqliteConnection, case: &CaseRow) -> AppResult<Vec<(StepDef, String)>> {
    let def = crate::services::definition::load_for_case(tx, case).await?;
    Ok(def
        .workflow
        .steps
        .into_iter()
        .filter_map(|s| {
            let t = s.handler.as_deref()?.strip_prefix("documents.letter_issued:")?.to_owned();
            Some((s, t))
        })
        .collect())
}
#[allow(clippy::too_many_arguments)]
pub async fn issue_letter(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case_id: i64,
    letter_type: &str,
    title: &str,
    body: &str,
) -> AppResult<DocumentId> {
    if !LETTER_TYPES.contains(&letter_type) {
        return Err(AppError::field("letter_type", "Choose a response letter required by this service."));
    }
    let case = super::manage(tx, actor, case_id, LETTER_ROLES).await?;
    let steps = letter_steps(tx, &case).await?;
    if !steps.iter().any(|(_, t)| t == letter_type) {
        return Err(AppError::field("letter_type", "This service has no workflow step for that letter."));
    }
    if !matches!(case.status.as_str(), "submitted" | "in_progress" | "waiting_on_applicant") {
        return Err(AppError::conflict("Response letters can only be issued while the case is open."));
    }
    // A letter belongs to the step run that waits for it: it is accepted only at that step and satisfies only it.
    let Some((step, _)) =
        steps.iter().find(|(s, t)| t == letter_type && case.current_step.as_deref() == Some(s.key.as_str()))
    else {
        return Err(AppError::conflict(format!(
            "The case has not reached a step waiting for the {} letter. Issue it when the case reaches that step.",
            letter_label(letter_type)
        )));
    };
    let run: i64 = sqlx::query_scalar(
        "SELECT id FROM workflow_step_runs WHERE case_id=? AND step_key=? AND left_at IS NULL ORDER BY id DESC LIMIT 1",
    )
    .bind(case_id)
    .bind(&step.key)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::conflict("No active workflow step."))?;
    if letter_issued(tx, case_id, letter_type).await? {
        return Err(AppError::conflict(format!(
            "The {} letter has already been issued for this step.",
            letter_label(letter_type)
        )));
    }
    super::text("title", title, 200)?;
    super::text("body", body, 20000)?;
    let bytes = crate::pdf::simple_document(
        title,
        &[("Case", case.number.clone().unwrap_or_default())],
        &[("Response", body.into())],
    );
    let (id, vid) =
        attach_generated(tx, state, case_id, "letter", title, Visibility::Applicant, bytes, actor.db_id()).await?;
    sqlx::query("INSERT INTO issued_letters(case_id,letter_type,document_id,document_version_id,issued_by,issued_at,step_run_id) VALUES(?,?,?,?,?,?,?)").bind(case_id).bind(letter_type).bind(id).bind(vid).bind(actor.db_id()).bind(time::now_str()).bind(run).execute(&mut *tx).await?;
    super::changed(
        tx,
        actor.db_id(),
        case_id,
        "documents.letter_issued",
        Visibility::Applicant,
        &format!("{title} has been issued — download it."),
    )
    .await?;
    super::notify_applicant(tx, &case, "Your response has been issued — download it", &format!("{title}\n\n{body}"))
        .await?;
    crate::cases::workflow::try_auto_advance(tx, state, case_id).await?;
    Ok(id)
}
/// Whether a letter of `letter_type` was issued for the case's current (open) workflow step run. Guards are
/// only evaluated for the current step, so a letter issued at an earlier step or run never satisfies a later one.
pub async fn letter_issued(tx: &mut SqliteConnection, case_id: i64, letter_type: &str) -> AppResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM issued_letters l JOIN workflow_step_runs r ON r.id=l.step_run_id \
         WHERE l.case_id=? AND l.letter_type=? AND r.left_at IS NULL)",
    )
    .bind(case_id)
    .bind(letter_type)
    .fetch_one(tx)
    .await?)
}
/// A letter issued before letters were tied to step runs (`step_run_id IS NULL`, migration 0805) was issued
/// ahead of its step under the old rules. It belongs to the first run of a step waiting for that letter type
/// entered after it: link it to `step_run_id` (called when such a step is entered), once only.
pub async fn adopt_unlinked_letter(
    tx: &mut SqliteConnection,
    case_id: i64,
    letter_type: &str,
    step_run_id: i64,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE issued_letters SET step_run_id=? WHERE id=(SELECT id FROM issued_letters WHERE case_id=? AND letter_type=? \
         AND step_run_id IS NULL ORDER BY id LIMIT 1) \
         AND NOT EXISTS(SELECT 1 FROM issued_letters WHERE step_run_id=? AND letter_type=?)",
    )
    .bind(step_run_id)
    .bind(case_id)
    .bind(letter_type)
    .bind(step_run_id)
    .bind(letter_type)
    .execute(&mut *tx)
    .await?;
    Ok(())
}
/// Whether the open case sits at a step waiting for a `letter_type` letter not yet issued for that step's run,
/// i.e. whether [`issue_letter`] would accept that letter now (role checks aside).
pub async fn letter_due(tx: &mut SqliteConnection, case: &CaseRow, letter_type: &str) -> AppResult<bool> {
    if !matches!(case.status.as_str(), "submitted" | "in_progress" | "waiting_on_applicant") {
        return Ok(false);
    }
    let at_step = letter_steps(tx, case)
        .await?
        .iter()
        .any(|(s, t)| t == letter_type && case.current_step.as_deref() == Some(s.key.as_str()));
    Ok(at_step && !letter_issued(tx, case.id, letter_type).await?)
}

/// Mark files disposed while retaining exact document/version/evidence IDs and blob metadata.
pub async fn dispose_case_files(tx: &mut SqliteConnection, case_id: i64) -> AppResult<Vec<String>> {
    crate::cases::core::load_case(tx, case_id).await?;
    sqlx::query("UPDATE documents SET disposed_at=? WHERE case_id=? AND disposed_at IS NULL")
        .bind(time::now_str())
        .bind(case_id)
        .execute(&mut *tx)
        .await?;
    let eligible = storage::disposed_orphan_hashes(tx).await?;
    let hashes: Vec<String> = sqlx::query_scalar("SELECT DISTINCT b.sha256 FROM document_versions v JOIN documents d ON d.id=v.document_id JOIN blobs b ON b.id=v.blob_id WHERE d.case_id=?")
        .bind(case_id).fetch_all(tx).await?;
    Ok(hashes.into_iter().filter(|hash| eligible.contains(hash)).collect())
}
