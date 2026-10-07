//! Shared application state handed to every handler (`State<AppState>`), job and seed function.

use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;

use crate::clock::{Clock, SystemClock};
use crate::config::Config;
use crate::error::AppResult;
use crate::rate_limit::RateLimiter;

/// Cheap to clone (all fields are reference-counted).
#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub cfg: Arc<Config>,
    pub clock: Arc<dyn Clock>,
    /// Outbound HTTP client (mock gateways, records systems, webhooks).
    pub http: reqwest::Client,
    pub rate: Arc<RateLimiter>,
}

impl AppState {
    /// Opens the database under `cfg.data_dir` and builds the state with the system clock.
    /// Does not run migrations (see `db::migrate`).
    pub async fn new(cfg: Config) -> AppResult<AppState> {
        Self::with_clock(cfg, Arc::new(SystemClock)).await
    }

    /// Same as [`AppState::new`] with an explicit clock (tests use `FixedClock`).
    pub async fn with_clock(cfg: Config, clock: Arc<dyn Clock>) -> AppResult<AppState> {
        std::fs::create_dir_all(&cfg.data_dir)?;
        let db = crate::db::connect(&cfg.db_path()).await?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| crate::error::AppError::internal(format!("http client: {e}")))?;
        Ok(AppState { db, cfg: Arc::new(cfg), clock, http, rate: Arc::new(RateLimiter::default()) })
    }

    /// Current instant from the configured clock.
    pub fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.clock.now()
    }
}

/// Test helpers shared by unit tests of all modules.
#[cfg(test)]
pub mod test_support {
    use super::*;
    use crate::clock::FixedClock;

    /// A migrated, empty database in a fresh temp dir. Keep the `TempDir` alive for the test's duration.
    pub async fn test_state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = Config::for_tests(dir.path());
        let state = AppState::new(cfg).await.expect("state");
        crate::db::migrate(&state.db).await.expect("migrate");
        (state, dir)
    }

    /// Like [`test_state`] with a [`FixedClock`] the test can move.
    pub async fn test_state_fixed(
        now: chrono::DateTime<chrono::Utc>,
    ) -> (AppState, Arc<FixedClock>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = Config::for_tests(dir.path());
        let clock = Arc::new(FixedClock::new(now));
        let state = AppState::with_clock(cfg, clock.clone()).await.expect("state");
        crate::db::migrate(&state.db).await.expect("migrate");
        (state, clock, dir)
    }
}
