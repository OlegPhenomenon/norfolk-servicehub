use servicehub::{
    AppState,
    clock::{self, FixedClock},
    config::Config,
    db,
    seed::{self, driver::Driver},
    time,
};
use std::{
    path::Path,
    sync::{Arc, OnceLock},
};

// Provider webhook limits are process-wide. Keep independent journeys from exhausting each other.
static JOURNEYS: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
pub struct FixtureDir {
    dir: tempfile::TempDir,
    _guard: tokio::sync::OwnedMutexGuard<()>,
}
impl FixtureDir {
    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

pub async fn fixture() -> (Driver, FixtureDir) {
    let guard = JOURNEYS.get_or_init(|| Arc::new(tokio::sync::Mutex::new(()))).clone().lock_owned().await;
    let dir = tempfile::tempdir().unwrap();
    let now = time::parse("2026-10-07T03:15:00Z").unwrap();
    let state = AppState::with_clock(Config::for_tests(dir.path()), Arc::new(FixedClock::new(now))).await.unwrap();
    db::migrate(&state.db).await.unwrap();
    clock::scope(state.clock.clone(), seed::seed_base(&state)).await.unwrap();
    let mut driver = Driver::new(state, now).await.unwrap();
    for who in ["alexey", "ben", "olga", "priya", "tom", "jake", "helen", "ruth", "mark"] {
        driver.login(who).await.unwrap();
    }
    (driver, FixtureDir { dir, _guard: guard })
}
