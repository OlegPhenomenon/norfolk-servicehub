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
