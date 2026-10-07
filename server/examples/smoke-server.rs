//! HTTP smoke harness. Time travel stays outside the production binary and HTTP API.
use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use chrono::{DateTime, Duration, Utc};
use servicehub::{AppState, app, clock::Clock, config::Config, db, jobs};

#[derive(Debug)]
struct SmokeClock(PathBuf);

impl Clock for SmokeClock {
    fn now(&self) -> DateTime<Utc> {
        let offset: i64 = std::fs::read_to_string(&self.0)
            .expect("smoke clock file")
            .trim()
            .parse()
            .expect("smoke clock offset in seconds");
        Utc::now() + Duration::seconds(offset)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
    let cfg = Config::from_env()?;
    anyhow::ensure!(cfg.demo_mode, "The smoke harness requires DEMO_MODE=true");
    let file = PathBuf::from(std::env::var("SMOKE_CLOCK_FILE")?);
    let state = AppState::with_clock(cfg, Arc::new(SmokeClock(file))).await.map_err(|e| anyhow::anyhow!(e.message))?;
    db::migrate(&state.db).await.map_err(|e| anyhow::anyhow!(e.message))?;
    tokio::spawn(jobs::run_worker(state.clone()));
    tokio::spawn(jobs::run_scheduler(state.clone()));
    let addr = SocketAddr::from(([127, 0, 0, 1], state.cfg.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app::build_router(state).into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}
