// OWNER: records
#![allow(dead_code, unused_variables)]
//! Backups (`servicehub backup <dir>`) and restore checks (`servicehub restore-check <dir>`).

use std::path::Path;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Writes a consistent backup (database + blobs) into `dir`.
pub async fn backup(state: &AppState, dir: &Path) -> AppResult<()> {
    Err(AppError::internal("not implemented: records::backup::backup"))
}

/// Restores a backup from `dir` into a scratch location and verifies it.
pub async fn restore_check(state: &AppState, dir: &Path) -> AppResult<()> {
    Err(AppError::internal("not implemented: records::backup::restore_check"))
}
