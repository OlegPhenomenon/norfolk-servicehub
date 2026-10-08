//! Per-module hook dispatch (owner: platform). The workflow engine and submission call only these
//! functions; they route to the module that owns the case's frozen `cases.module`:
//!
//! | `cases.module` | hooks file |
//! |---|---|
//! | `venue_booking`, `equipment_hire`, `road_issue` | `operations/hooks.rs` |
//! | `building`, `planning_certificate` | `documents/hooks.rs` |
//! | `complaint` | `records/hooks.rs` |
//! | `generic` | none (no-op; pricing from the definition) |
//!
//! `module` workflow steps are dispatched on `step.handler` prefix instead (`operations.*`, `finance.*`,
//! `documents.*`) to `<owner>::hooks::step_guard_handler`.

use chrono::NaiveDate;
use serde_json::Value;
use sqlx::SqliteConnection;

use crate::auth::Actor;
use crate::cases::core::CaseRow;
use crate::error::{AppError, AppResult};
use crate::finance::api::QuoteLine;
use crate::services::definition::{FieldDef, FieldType, StepDef, StepKind};
use crate::state::AppState;
use crate::{documents, finance, operations, records, time};

/// Module family that owns a case's hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookOwner {
    Generic,
    Operations,
    Documents,
    Records,
}

/// Maps `cases.module` / `services.module` to the owning hooks.
pub fn owner_of(module: &str) -> HookOwner {
    match module {
        "venue_booking" | "equipment_hire" | "road_issue" => HookOwner::Operations,
        "building" | "planning_certificate" => HookOwner::Documents,
        "complaint" => HookOwner::Records,
        _ => HookOwner::Generic,
    }
}

/// Module-specific field validation. The case module's owner is asked first; custom field types owned by
/// another slice (`booking_slot`/`equipment_request` → operations, `decision_ref` → documents) are then
/// validated by their owner even in services of other modules. `Some(message)` = invalid.
pub async fn validate_field(
    tx: &mut SqliteConnection,
    module: &str,
    field: &FieldDef,
    value: &Value,
) -> AppResult<Option<String>> {
    let owner = owner_of(module);
    let first = match owner {
        HookOwner::Operations => operations::hooks::validate_field(tx, module, field, value).await?,
        HookOwner::Documents => documents::hooks::validate_field(tx, module, field, value).await?,
        HookOwner::Records => records::hooks::validate_field(tx, module, field, value).await?,
        HookOwner::Generic => None,
    };
    if first.is_some() {
        return Ok(first);
    }
    match field.field_type {
        FieldType::BookingSlot | FieldType::EquipmentRequest if owner != HookOwner::Operations => {
            operations::hooks::validate_field(tx, module, field, value).await
        }
        FieldType::DecisionRef if owner != HookOwner::Documents => {
            documents::hooks::validate_field(tx, module, field, value).await
        }
        _ => Ok(None),
    }
}

/// Called inside the submission transaction once the case is numbered and the snapshot stored.
pub async fn on_submit(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: &CaseRow,
    answers: &Value,
) -> AppResult<()> {
    match owner_of(&case.module) {
        HookOwner::Operations => operations::hooks::on_submit(tx, state, actor, case, answers).await,
        HookOwner::Documents => documents::hooks::on_submit(tx, state, actor, case, answers).await,
        HookOwner::Records => records::hooks::on_submit(tx, state, actor, case, answers).await,
        HookOwner::Generic => Ok(()),
    }
}

/// Called when the case enters `step` (a new `workflow_step_runs` row `step_run_id` exists).
///
/// Generic behaviour by step kind, then the module hook:
/// * `payment` → `finance::api::ensure_invoice_for_step` (prices via [`pricing_lines`]);
/// * `task` → `operations::api::create_step_task`.
pub async fn on_step_entered(
    tx: &mut SqliteConnection,
    state: &AppState,
    actor: &Actor,
    case: &CaseRow,
    step: &StepDef,
    step_run_id: i64,
) -> AppResult<()> {
    match step.kind {
        StepKind::Payment => {
            if finance::api::ensure_invoice_for_step(tx, state, actor, case).await?.is_some() {
                finance::building_fees::invoice_issued(tx, case).await?;
            }
        }
        StepKind::Task => {
            operations::api::create_step_task(tx, case, step, step_run_id, actor.db_id()).await?;
        }
        StepKind::Module => {
            if let Some(letter_type) = step.handler.as_deref().and_then(|h| h.strip_prefix("documents.letter_issued:"))
            {
                documents::api::adopt_unlinked_letter(tx, case.id, letter_type, step_run_id).await?;
            }
        }
        _ => {}
    }
    match owner_of(&case.module) {
        HookOwner::Operations => operations::hooks::on_step_entered(tx, state, actor, case, step, step_run_id).await,
        HookOwner::Documents => documents::hooks::on_step_entered(tx, state, actor, case, step, step_run_id).await,
        HookOwner::Records => records::hooks::on_step_entered(tx, state, actor, case, step, step_run_id).await,
        HookOwner::Generic => Ok(()),
    }
}

/// Called before a staff `skip` of `step`. `Some(reason)` blocks the skip. Building cases never leave an open
/// exhibition or unconsidered comments behind; a skipped exhibition step is recorded as "not required".
pub async fn on_skip(
    tx: &mut SqliteConnection,
    actor: &Actor,
    case: &CaseRow,
    step: &StepDef,
    reason: &str,
) -> AppResult<Option<String>> {
    match owner_of(&case.module) {
        HookOwner::Documents => documents::hooks::on_skip(tx, actor, case, step, reason).await,
        _ => Ok(None),
    }
}

/// The guard to leave `step` (ARCHITECTURE §5). `Some(reason)` blocks, `None` lets the case advance.
///
/// * `review` / `complete`: no automatic guard (staff decide).
/// * `payment`: `finance::api::case_settled`.
/// * `decision`: the documents module names the required issued decisions — for building approval routes the
///   case's confirmed scope (DA, BA, both, or one modification per original), else `decision_types`
///   (refusal routing is the workflow's job).
/// * `task`: all tasks of the current open step run are done.
/// * `module`: dispatched on `step.handler` prefix to `<owner>::hooks::step_guard_handler`.
///
/// For non-`module` steps the case module's `step_guard` may add an extra block.
pub async fn step_guard(tx: &mut SqliteConnection, case: &CaseRow, step: &StepDef) -> AppResult<Option<String>> {
    let generic = match step.kind {
        StepKind::Module => {
            let handler = step
                .handler
                .as_deref()
                .ok_or_else(|| AppError::internal(format!("module step {} has no handler", step.key)))?;
            return if handler.starts_with("operations.") {
                operations::hooks::step_guard_handler(tx, case, handler).await
            } else if handler.starts_with("finance.") {
                finance::hooks::step_guard_handler(tx, case, handler).await
            } else if handler.starts_with("documents.") {
                documents::hooks::step_guard_handler(tx, case, handler).await
            } else {
                Err(AppError::internal(format!("unknown step handler {handler}")))
            };
        }
        StepKind::Review | StepKind::Complete => None,
        StepKind::Payment => {
            (!finance::api::case_settled(tx, case.id).await?).then(|| "Payment has not been received yet.".to_string())
        }
        StepKind::Decision => documents::building::decision_guard(tx, case, step).await?,
        StepKind::Task => {
            let run: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM workflow_step_runs WHERE case_id = ? AND step_key = ? AND left_at IS NULL ORDER BY id DESC LIMIT 1",
            )
            .bind(case.id)
            .bind(&step.key)
            .fetch_optional(&mut *tx)
            .await?;
            match run {
                None => Some("This step has not started yet.".to_string()),
                Some(run_id) => (!operations::api::step_tasks_done(tx, case.id, run_id).await?)
                    .then(|| "The field task for this step is not finished yet.".to_string()),
            }
        }
    };
    if generic.is_some() {
        return Ok(generic);
    }
    match owner_of(&case.module) {
        HookOwner::Operations => operations::hooks::step_guard(tx, case, step).await,
        HookOwner::Documents => documents::hooks::step_guard(tx, case, step).await,
        HookOwner::Records => records::hooks::step_guard(tx, case, step).await,
        HookOwner::Generic => Ok(None),
    }
}

/// Pricing date and lines for the case's invoice. Module owners may price themselves (venue: event date;
/// equipment: date of actual use); otherwise the definition's `pricing` at the submission date
/// (`finance::api::definition_pricing_lines`).
pub async fn pricing_lines(tx: &mut SqliteConnection, case: &CaseRow) -> AppResult<(NaiveDate, Vec<QuoteLine>)> {
    let custom = match owner_of(&case.module) {
        HookOwner::Operations => operations::hooks::pricing_lines(tx, case).await?,
        HookOwner::Documents => documents::hooks::pricing_lines(tx, case).await?,
        HookOwner::Records => records::hooks::pricing_lines(tx, case).await?,
        HookOwner::Generic => None,
    };
    if let Some(p) = custom {
        return Ok(p);
    }
    let submitted = case.submitted_at.as_deref().unwrap_or(&case.created_at);
    let pricing_date = time::local_date(time::parse(submitted)?);
    let lines = finance::api::definition_pricing_lines(tx, case).await?;
    Ok((pricing_date, lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_owners() {
        assert_eq!(owner_of("venue_booking"), HookOwner::Operations);
        assert_eq!(owner_of("equipment_hire"), HookOwner::Operations);
        assert_eq!(owner_of("road_issue"), HookOwner::Operations);
        assert_eq!(owner_of("building"), HookOwner::Documents);
        assert_eq!(owner_of("planning_certificate"), HookOwner::Documents);
        assert_eq!(owner_of("complaint"), HookOwner::Records);
        assert_eq!(owner_of("generic"), HookOwner::Generic);
    }
}
