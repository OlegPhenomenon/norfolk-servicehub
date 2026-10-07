//! Demo data: `wipe` (delete every row except migrations) and `seed_demo` (personas, roles, decision
//! authority, organisation, holidays, settings, then each module's `seed`, then `scenarios::run`).
//!
//! Module seeds run inside one write transaction and must not touch the pool (stage blobs with
//! `storage::stage` — it does not use the database). `scenarios::run` runs after commit and opens its own
//! transactions through the same domain functions as live actions.

#[doc(hidden)]
pub mod driver;
pub mod scenarios;

use std::path::Path;
use std::time::Duration;

use sqlx::SqliteConnection;

use crate::auth::demo::{DEMO_ORGANISATION, PERSONAS};
use crate::auth::users::{self, NewUser};
use crate::auth::{UserKind, totp};
use crate::db::write_tx;
use crate::error::{AppError, AppResult};
use crate::settings::{self, keys};
use crate::state::AppState;
use crate::{storage, time};

/// Decision types Priya may issue (granted by Helen).
pub const PRIYA_DECISION_TYPES: [&str; 4] =
    ["development_approval", "building_approval", "planning_certificate", "modification_approval"];

/// Deletes all rows of all tables except `_sqlx_migrations` (FTS tables via `DELETE`, their shadow
/// tables untouched) in one transaction, then removes every blob file.
pub async fn wipe(state: &AppState) -> AppResult<()> {
    let mut tx = write_tx(&state.db).await?;
    sqlx::query("PRAGMA defer_foreign_keys = ON").execute(&mut *tx).await?;
    let tables: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT name, sql FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations'",
    )
    .fetch_all(&mut *tx)
    .await?;
    let virtual_tables: Vec<&str> = tables
        .iter()
        .filter(|(_, sql)| {
            sql.as_deref().is_some_and(|s| s.trim_start().to_ascii_uppercase().starts_with("CREATE VIRTUAL TABLE"))
        })
        .map(|(n, _)| n.as_str())
        .collect();
    for (name, _) in &tables {
        let is_shadow = virtual_tables.iter().any(|v| name.starts_with(&format!("{v}_")));
        if is_shadow {
            continue;
        }
        sqlx::query(&format!("DELETE FROM \"{}\"", name.replace('"', "\"\""))).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    storage::gc_older_than(state, Duration::ZERO).await?;
    Ok(())
}

/// Seeds the base demo data and then every module's seed and the scenarios. Expects an empty database.
pub async fn seed_demo(state: &AppState) -> AppResult<()> {
    seed_base(state).await?;
    Box::pin(scenarios::run(state)).await?;
    Ok(())
}

/// Platform and catalogue only, for isolated acceptance fixtures.
#[doc(hidden)]
pub async fn seed_base(state: &AppState) -> AppResult<()> {
    let mut tx = write_tx(&state.db).await?;
    seed_platform(&mut tx, state).await?;
    crate::services::seed(&mut tx, state).await?;
    crate::documents::seed(&mut tx, state).await?;
    crate::operations::seed(&mut tx, state).await?;
    crate::finance::seed(&mut tx, state).await?;
    crate::records::seed(&mut tx, state).await?;
    tx.commit().await?;
    Ok(())
}

/// Wipe + seed (CLI `seed-demo` / `reset-demo` and the scheduled `demo.reset` job).
pub async fn reset_demo(state: &AppState) -> AppResult<()> {
    wipe(state).await?;
    seed_demo(state).await?;
    tracing::info!("demo data reset");
    Ok(())
}

async fn user_id_by_persona(tx: &mut SqliteConnection, key: &str) -> AppResult<i64> {
    sqlx::query_scalar("SELECT id FROM users WHERE persona_key = ?")
        .bind(key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::internal(format!("persona {key} missing")))
}

async fn seed_platform(tx: &mut SqliteConnection, state: &AppState) -> AppResult<()> {
    // Personas.
    for p in PERSONAS {
        let staff = p.kind == UserKind::Staff;
        let id = users::create_user(
            tx,
            NewUser {
                email: p.email(),
                display_name: p.name.to_string(),
                phone: p.phone.map(String::from),
                kind: p.kind,
                password_hash: None,
                totp_secret: staff.then(totp::generate_secret),
                totp_enabled: staff,
                persona_key: Some(p.key.to_string()),
                job_title: p.job_title.map(String::from),
            },
        )
        .await?;
        for role in p.roles {
            users::grant_role(tx, id, *role, None, None).await?;
        }
    }

    // Decision authority: Helen (manager) grants Priya.
    let helen = user_id_by_persona(tx, "helen").await?;
    let priya = user_id_by_persona(tx, "priya").await?;
    for t in PRIYA_DECISION_TYPES {
        users::grant_decision_authority(tx, priya, t, None, helen).await?;
    }

    // Organisation with Ben as active owner.
    let ben = user_id_by_persona(tx, "ben").await?;
    let now = time::now_str();
    let org_id: i64 =
        sqlx::query_scalar("INSERT INTO organisations (name, abn, created_at) VALUES (?, NULL, ?) RETURNING id")
            .bind(DEMO_ORGANISATION)
            .bind(&now)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query(
        "INSERT INTO memberships (organisation_id, user_id, invite_email, role, status, created_at, accepted_at) \
         VALUES (?, ?, ?, 'owner', 'active', ?, ?)",
    )
    .bind(org_id)
    .bind(ben)
    .bind("ben@demo.servicehub.invalid")
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    // Holidays.
    let n = load_holidays(tx, &state.cfg.seed_data_dir.join("holidays.csv")).await?;
    tracing::info!(holidays = n, "seeded holidays");

    // Settings.
    settings::set(tx, keys::CUSTOMER_CARE_EMAIL, &"customercare@demo.servicehub.invalid", None).await?;
    settings::set(tx, keys::DEMO_RESET_HOURS, &state.cfg.demo_reset_hours, None).await?;
    settings::set(tx, keys::DEMO_LAST_RESET_AT, &time::fmt(state.now()), None).await?;
    Ok(())
}

/// Splits one CSV line (RFC 4180 quotes, `""` escapes).
fn split_csv_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).collect()
}

/// Loads `date,name,source` rows into `holidays` (calendar `norfolk`). A missing file logs a warning and
/// seeds nothing; malformed rows are an error. Returns the number of rows inserted.
pub async fn load_holidays(tx: &mut SqliteConnection, path: &Path) -> AppResult<usize> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(path = %path.display(), "holidays.csv not found; seeding no holidays");
            return Ok(0);
        }
        Err(e) => return Err(e.into()),
    };
    let mut n = 0;
    for (i, line) in text.lines().enumerate() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols = split_csv_line(line);
        if i == 0 && cols.first().is_some_and(|c| c.eq_ignore_ascii_case("date")) {
            continue;
        }
        let [date, name, source] = cols.as_slice() else {
            return Err(AppError::internal(format!("{}:{}: expected date,name,source", path.display(), i + 1)));
        };
        let date = time::parse_date(date)
            .map_err(|_| AppError::internal(format!("{}:{}: bad date {date}", path.display(), i + 1)))?;
        sqlx::query(
            "INSERT INTO holidays (calendar, date, name, source) VALUES ('norfolk', ?, ?, ?) \
             ON CONFLICT(calendar, date) DO UPDATE SET name = excluded.name, source = excluded.source",
        )
        .bind(time::fmt_date(date))
        .bind(name)
        .bind(source)
        .execute(&mut *tx)
        .await?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn counts(state: &AppState) -> Vec<(String, i64)> {
        let mut out = Vec::new();
        for t in [
            "users",
            "role_grants",
            "decision_authorities",
            "organisations",
            "memberships",
            "settings",
            "holidays",
            "sessions",
            "notifications",
            "jobs",
        ] {
            let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {t}")).fetch_one(&state.db).await.unwrap();
            out.push((t.to_string(), n));
        }
        out
    }

    #[test]
    fn csv_lines() {
        assert_eq!(
            split_csv_line(r#"2026-06-08,"Bounty Day, observed",demo"#),
            vec!["2026-06-08", "Bounty Day, observed", "demo"]
        );
        assert_eq!(split_csv_line(r#"a,"say ""hi""",c"#), vec!["a", "say \"hi\"", "c"]);
    }

    #[tokio::test]
    async fn wipe_and_seed_twice_is_clean() {
        let (mut state, dir) = crate::state::test_support::test_state().await;
        // Use a private seed-data dir with a small holidays file.
        let seed_dir = dir.path().join("seed-data");
        std::fs::create_dir_all(&seed_dir).unwrap();
        std::fs::write(
            seed_dir.join("holidays.csv"),
            "date,name,source\n2026-06-08,\"Bounty Day\",demo\n2026-12-25,Christmas Day,demo\n",
        )
        .unwrap();
        let mut cfg = (*state.cfg).clone();
        cfg.seed_data_dir = seed_dir;
        state.cfg = std::sync::Arc::new(cfg);

        reset_demo(&state).await.unwrap();
        let first = counts(&state).await;
        // Leave some extra state behind (a session and a blob) — the reset must remove it.
        let mut tx = write_tx(&state.db).await.unwrap();
        let alexey = user_id_by_persona(&mut tx, "alexey").await.unwrap();
        crate::auth::session::create(&mut tx, alexey, true, state.now()).await.unwrap();
        tx.commit().await.unwrap();
        let blob = storage::put(&state, b"%PDF-1.4\n%%EOF\n", "x.pdf", storage::AllowList::Docs, None).await.unwrap();

        reset_demo(&state).await.unwrap();
        let second = counts(&state).await;
        assert_eq!(first, second);
        assert!(!storage::blob_path(&state.cfg.blobs_dir(), &blob.sha256).exists());

        let get = |t: &str| second.iter().find(|(n, _)| n == t).unwrap().1;
        assert_eq!(get("users"), 9);
        assert_eq!(get("role_grants"), 7);
        assert_eq!(get("decision_authorities"), 4);
        assert_eq!(get("memberships"), 1);
        assert_eq!(get("holidays"), 2);
        assert_eq!(get("sessions"), 0);
        let staff_with_totp: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM users WHERE kind = 'staff' AND totp_enabled = 1 AND totp_secret IS NOT NULL",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(staff_with_totp, 7);
        let fk_errors: Vec<(String,)> =
            sqlx::query_as("SELECT \"table\" FROM pragma_foreign_key_check").fetch_all(&state.db).await.unwrap();
        assert!(fk_errors.is_empty(), "{fk_errors:?}");
    }

    #[tokio::test]
    async fn missing_holidays_file_is_a_warning() {
        let (state, dir) = crate::state::test_support::test_state().await;
        let mut tx = write_tx(&state.db).await.unwrap();
        assert_eq!(load_holidays(&mut tx, &dir.path().join("nope.csv")).await.unwrap(), 0);
    }
}
