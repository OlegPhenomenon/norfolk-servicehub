//! `servicehub serve | migrate | seed-demo | reset-demo | backup <dir> | restore-check <backup-dir>`

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use servicehub::config::Config;
use servicehub::state::AppState;
use servicehub::{app, db, records, seed};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "servicehub", version, about = "Norfolk ServiceHub — council services demo")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run migrations, then serve HTTP with the background worker and scheduler.
    Serve,
    /// Apply database migrations and exit.
    Migrate,
    /// Migrate, wipe all data and seed the demo.
    SeedDemo,
    /// Same as seed-demo.
    ResetDemo,
    /// Write a backup (database + blobs) into DIR.
    Backup { dir: PathBuf },
    /// Verify that a backup in DIR restores cleanly.
    RestoreCheck { dir: PathBuf },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,tower_http=info")),
        )
        .init();
    let cli = Cli::parse();
    let cfg = Config::from_env()?;
    let state = AppState::new(cfg).await.map_err(|e| anyhow::anyhow!("{}", e.message))?;
    let run = async {
        match cli.command {
            Command::Serve => {
                db::migrate(&state.db).await?;
                app::serve(state.clone()).await.map_err(servicehub::AppError::from)?;
            }
            Command::Migrate => {
                db::migrate(&state.db).await?;
                tracing::info!("migrations applied");
            }
            Command::SeedDemo | Command::ResetDemo => {
                db::migrate(&state.db).await?;
                seed::reset_demo(&state).await?;
            }
            Command::Backup { dir } => records::backup::backup(&state, &dir).await?,
            Command::RestoreCheck { dir } => records::backup::restore_check(&state, &dir).await?,
        }
        Ok::<(), servicehub::AppError>(())
    };
    run.await.map_err(|e| anyhow::anyhow!("{}", e.message))?;
    state.db.close().await;
    Ok(())
}
