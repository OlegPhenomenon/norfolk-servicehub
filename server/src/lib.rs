//! Norfolk ServiceHub server library. See `docs/ARCHITECTURE.md` for the module map and conventions.

// Platform modules.
pub mod app;
pub mod audit;
pub mod auth;
pub mod authz;
pub mod calendar;
pub mod clock;
pub mod config;
pub mod db;
pub mod error;
pub mod hooks;
pub mod idempotency;
pub mod jobs;
pub mod notify;
pub mod pdf;
pub mod rate_limit;
pub mod settings;
pub mod state;
pub mod storage;
pub mod time;
pub mod web;

// Cases: `cases/core.rs` is platform, the rest belongs to services.
pub mod cases;

// Domain modules (one owner each).
pub mod deadlines;
pub mod documents;
pub mod finance;
pub mod mock;
pub mod operations;
pub mod records;
pub mod seed;
pub mod services;

pub use error::{AppError, AppResult};
pub use state::AppState;
