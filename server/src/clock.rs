//! Time source abstraction. Code that reasons about "now" (deadlines, jobs, sessions) should take it
//! from `AppState::clock` so tests can use [`FixedClock`].

use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};

/// Source of the current instant.
pub trait Clock: Send + Sync + std::fmt::Debug {
    fn now(&self) -> DateTime<Utc>;
}

/// The real wall clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A settable clock for tests.
#[derive(Debug)]
pub struct FixedClock {
    now: Mutex<DateTime<Utc>>,
}

impl FixedClock {
    pub fn new(now: DateTime<Utc>) -> Self {
        FixedClock { now: Mutex::new(now) }
    }

    /// Sets the current instant.
    pub fn set(&self, now: DateTime<Utc>) {
        *self.now.lock().expect("clock mutex") = now;
    }

    /// Moves the clock forward.
    pub fn advance(&self, by: Duration) {
        let mut guard = self.now.lock().expect("clock mutex");
        *guard += by;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().expect("clock mutex")
    }
}

// Restricted to an explicitly scoped seed/test future; concurrent requests keep their own clock.
tokio::task_local! {
    static OVERRIDE: std::sync::Arc<dyn Clock>;
}

/// Current time for domain APIs that have no AppState parameter.
pub fn now() -> DateTime<Utc> {
    OVERRIDE.try_with(|clock| clock.now()).unwrap_or_else(|_| Utc::now())
}

/// Run endpoint/domain commands with an explicit clock without changing process-global time.
pub async fn scope<F: std::future::Future>(clock: std::sync::Arc<dyn Clock>, future: F) -> F::Output {
    OVERRIDE.scope(clock, future).await
}
