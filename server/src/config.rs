//! Environment configuration (see `docs/ARCHITECTURE.md` §1 and `.env.example`).

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use rand::RngCore;

/// Process configuration, read once at start-up. Shared through `AppState::cfg`.
#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub data_dir: PathBuf,
    pub public_base_url: String,
    pub internal_base_url: String,
    pub web_dist: PathBuf,
    pub demo_mode: bool,
    /// 0 = never reset.
    pub demo_reset_hours: u32,
    /// Demo mode only: after this instant the site serves the "demonstration has ended" page.
    pub demo_ends_at: Option<DateTime<Utc>>,
    pub ai_enabled: bool,
    pub cookie_secure: bool,
    /// HMAC secret shared by DemoPay and the webhook handler.
    pub webhook_secret: String,
    /// Value of the `X-Mock-Key` header required by server-to-server `/mock/**` APIs.
    pub mock_api_key: String,
    /// Take the client IP from the first hop of `X-Forwarded-For`.
    pub trust_proxy: bool,
    /// Directory with seed CSV/JSON files (`holidays.csv`, …).
    pub seed_data_dir: PathBuf,
    /// Public source repository shown on the "demonstration has ended" page (optional).
    pub repo_url: Option<String>,
}

fn env_str(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn env_bool(key: &str, default: bool) -> anyhow::Result<bool> {
    match env_str(key) {
        None => Ok(default),
        Some(v) => match v.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => anyhow::bail!("{key} must be true or false, got {v:?}"),
        },
    }
}

fn random_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

impl Config {
    /// Reads the environment. Generates `WEBHOOK_SECRET` / `MOCK_API_KEY` when unset (logged, value never logged).
    pub fn from_env() -> anyhow::Result<Config> {
        let port: u16 = match env_str("PORT") {
            Some(p) => p.parse().map_err(|_| anyhow::anyhow!("PORT must be a number"))?,
            None => 8080,
        };
        let data_dir = PathBuf::from(env_str("DATA_DIR").unwrap_or_else(|| "./data".into()));
        let demo_ends_at = match env_str("DEMO_ENDS_AT") {
            None => None,
            Some(v) => Some(
                DateTime::parse_from_rfc3339(&v)
                    .map_err(|e| anyhow::anyhow!("DEMO_ENDS_AT must be RFC 3339: {e}"))?
                    .with_timezone(&Utc),
            ),
        };
        let webhook_secret = env_str("WEBHOOK_SECRET").unwrap_or_else(|| {
            tracing::info!("WEBHOOK_SECRET not set; generated a random secret for this process");
            random_secret()
        });
        let mock_api_key = env_str("MOCK_API_KEY").unwrap_or_else(|| {
            tracing::info!("MOCK_API_KEY not set; generated a random key for this process");
            random_secret()
        });
        let seed_data_dir = match env_str("SEED_DATA_DIR") {
            Some(d) => PathBuf::from(d),
            None => {
                let local = PathBuf::from("./seed-data");
                if local.is_dir() { local } else { PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/seed-data")) }
            }
        };
        Ok(Config {
            port,
            data_dir,
            public_base_url: env_str("PUBLIC_BASE_URL")
                .unwrap_or_else(|| "http://localhost:8080".into())
                .trim_end_matches('/')
                .to_string(),
            internal_base_url: env_str("INTERNAL_BASE_URL")
                .unwrap_or_else(|| format!("http://127.0.0.1:{port}"))
                .trim_end_matches('/')
                .to_string(),
            web_dist: PathBuf::from(env_str("WEB_DIST").unwrap_or_else(|| "../web/dist".into())),
            demo_mode: env_bool("DEMO_MODE", false)?,
            demo_reset_hours: match env_str("DEMO_RESET_HOURS") {
                Some(v) => v.parse().map_err(|_| anyhow::anyhow!("DEMO_RESET_HOURS must be a whole number"))?,
                None => 6,
            },
            demo_ends_at,
            ai_enabled: env_bool("AI_ENABLED", true)?,
            cookie_secure: env_bool("COOKIE_SECURE", true)?,
            webhook_secret,
            mock_api_key,
            trust_proxy: env_bool("TRUST_PROXY", true)?,
            seed_data_dir,
            repo_url: env_str("REPO_URL"),
        })
    }

    /// A configuration for tests: everything under `data_dir`, demo mode on, fixed secrets.
    pub fn for_tests(data_dir: impl Into<PathBuf>) -> Config {
        Config {
            port: 0,
            data_dir: data_dir.into(),
            public_base_url: "http://localhost:8080".into(),
            internal_base_url: "http://127.0.0.1:9".into(),
            web_dist: PathBuf::from("/nonexistent-web-dist"),
            demo_mode: true,
            demo_reset_hours: 6,
            demo_ends_at: None,
            ai_enabled: true,
            cookie_secure: false,
            webhook_secret: "test-webhook-secret".into(),
            mock_api_key: "test-mock-key".into(),
            trust_proxy: true,
            seed_data_dir: PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/seed-data")),
            repo_url: None,
        }
    }

    /// Path of the SQLite database file.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("servicehub.db")
    }

    /// Root directory of content-addressed blob files.
    pub fn blobs_dir(&self) -> PathBuf {
        self.data_dir.join("blobs")
    }

    /// True when the public demo has passed `DEMO_ENDS_AT` (demo mode only).
    pub fn demo_has_ended(&self, now: DateTime<Utc>) -> bool {
        self.demo_mode && self.demo_ends_at.is_some_and(|end| now >= end)
    }
}
