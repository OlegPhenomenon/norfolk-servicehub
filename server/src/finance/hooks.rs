// OWNER: finance
#![allow(dead_code, unused_variables)]
//! Finance step handlers (`finance.*`). Called only through `crate::hooks`.

use sqlx::SqliteConnection;

use crate::cases::core::CaseRow;
use crate::error::AppResult;

/// Guard of a `module` step with a `finance.*` handler (`finance.deposits_settled`). `Some(reason)` blocks.
pub async fn step_guard_handler(tx: &mut SqliteConnection, case: &CaseRow, handler: &str) -> AppResult<Option<String>> {
    Ok(None)
}
