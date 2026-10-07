// OWNER: documents
#![allow(dead_code, unused_variables)]
//! Cross-module documents API.

use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::auth::Actor;
use crate::cases::core::Visibility;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub type DocumentId = i64;
pub type DocumentVersionId = i64;

/// An issued (or refused) decision, as seen by the workflow guard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionSummary {
    pub id: i64,
    pub decision_type: String,
    /// `approved` | `approved_with_conditions` | `refused`.
    pub outcome: String,
    pub status: String,
    pub issued_at: Option<String>,
    pub output_document_version_id: Option<i64>,
}

/// Stores a generated PDF as a new case document (version 1). Stage the bytes with
/// `storage::stage` *before* opening `tx` if you generate them yourself — this function receives bytes and
/// must not touch the pool. `actor` = `users.id` or `None` for system-generated documents.
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
    Err(AppError::internal("not implemented: documents::api::attach_generated"))
}

/// Issued decisions of a case (all types).
pub async fn issued_decisions(tx: &mut SqliteConnection, case_id: i64) -> AppResult<Vec<DecisionSummary>> {
    Err(AppError::internal("not implemented: documents::api::issued_decisions"))
}

/// Issues a letter (`complaint_response` | `road_response`) as an applicant-visible PDF document.
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
    Err(AppError::internal("not implemented: documents::api::issue_letter"))
}

/// Has a letter of `letter_type` been issued for the case?
pub async fn letter_issued(tx: &mut SqliteConnection, case_id: i64, letter_type: &str) -> AppResult<bool> {
    Err(AppError::internal("not implemented: documents::api::letter_issued"))
}
