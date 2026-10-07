//! Consistent SQLite snapshots and verified, isolated restores.
//! VACUUM cannot execute in a transaction. A dedicated read connection performs VACUUM INTO
//! while a BEGIN IMMEDIATE transaction on the pool prevents all source writes.
use crate::{
    db,
    error::{AppError, AppResult},
    state::AppState,
    storage, time,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Manifest {
    app_version: String,
    created_at: String,
    counts: BTreeMap<String, i64>,
    blobs: BTreeMap<String, String>,
}
async fn table_counts(conn: &mut SqliteConnection) -> AppResult<BTreeMap<String, i64>> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut counts = BTreeMap::new();
    for table in tables {
        let quoted = table.replace('"', "\"\"");
        let count: i64 =
            sqlx::query_scalar(&format!("SELECT COUNT(*) FROM \"{quoted}\"")).fetch_one(&mut *conn).await?;
        counts.insert(table, count);
    }
    Ok(counts)
}
/// Discover all schema references, so other slices' new blob consumers are included automatically.
async fn blob_inventory(conn: &mut SqliteConnection) -> AppResult<Vec<String>> {
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
            .fetch_all(&mut *conn)
            .await?;
    let mut references = Vec::new();
    for table in tables {
        let quoted = table.replace('"', "\"\"");
        let foreign_keys = super::common::rows(conn, &format!("PRAGMA foreign_key_list(\"{quoted}\")"), &[]).await?;
        for key in foreign_keys {
            if key["table"] != "blobs" {
                continue;
            }
            let column = key["from"]
                .as_str()
                .ok_or_else(|| AppError::internal("Invalid foreign key metadata"))?
                .replace('"', "\"\"");
            if table == "document_versions" {
                references.push("EXISTS(SELECT 1 FROM document_versions v JOIN documents d ON d.id=v.document_id WHERE v.blob_id=b.id AND d.disposed_at IS NULL)".to_string());
            } else {
                references.push(format!("EXISTS(SELECT 1 FROM \"{quoted}\" r WHERE r.\"{column}\"=b.id)"));
            }
        }
    }
    let predicate = if references.is_empty() { "0".to_string() } else { references.join(" OR ") };
    Ok(sqlx::query_scalar(&format!("SELECT b.sha256 FROM blobs b WHERE {predicate} ORDER BY b.sha256"))
        .fetch_all(conn)
        .await?)
}
async fn start(state: &AppState, kind: &str, dir: &Path) -> AppResult<i64> {
    let mut tx = db::write_tx(&state.db).await?;
    let id = sqlx::query_scalar(
        "INSERT INTO backup_runs(kind,status,started_at,location) VALUES(?,'running',?,?) RETURNING id",
    )
    .bind(kind)
    .bind(time::fmt(state.now()))
    .bind(dir.to_string_lossy().as_ref())
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}
async fn finish(state: &AppState, id: i64, result: &AppResult<Value>) -> AppResult<()> {
    let mut tx = db::write_tx(&state.db).await?;
    let (status, details) = match result {
        Ok(v) => ("ok", v.clone()),
        Err(e) => ("failed", json!({"error":e.message})),
    };
    sqlx::query("UPDATE backup_runs SET status=?,finished_at=?,details_json=? WHERE id=?")
        .bind(status)
        .bind(time::fmt(state.now()))
        .bind(details.to_string())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    crate::audit::record(&mut tx, None, &format!("records.{status}"), "backup_run", Some(id), details).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn backup(state: &AppState, dir: &Path) -> AppResult<()> {
    let id = start(state, "backup", dir).await?;
    let result = snapshot(state, dir).await;
    finish(state, id, &result).await?;
    result.map(|_| ())
}
async fn snapshot(state: &AppState, dir: &Path) -> AppResult<Value> {
    std::fs::create_dir_all(dir)?;
    let destination = dir.join("servicehub.db");
    if destination.exists() || dir.join("manifest.json").exists() {
        return Err(AppError::conflict("Choose an empty backup directory."));
    }
    let source = state.cfg.db_path();
    // Acquire the dedicated reader before the writer, avoiding pool acquisition under a write lock.
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&source)
        .read_only(true)
        .busy_timeout(std::time::Duration::from_secs(5));
    let mut reader = SqliteConnection::connect_with(&options).await?;
    let guard = db::write_tx(&state.db).await?;
    let path = destination.to_string_lossy().replace('\'', "''");
    sqlx::query(&format!("VACUUM INTO '{path}'")).execute(&mut reader).await?;
    let counts = table_counts(&mut reader).await?;
    let hashes: Vec<String> = blob_inventory(&mut reader).await?;
    let mut blobs = BTreeMap::new();
    for hash in hashes {
        validate_hash(&hash)?;
        let bytes = std::fs::read(storage::blob_path(&state.cfg.blobs_dir(), &hash))?;
        if hex::encode(Sha256::digest(&bytes)) != hash {
            return Err(AppError::internal(format!("Source blob {hash} is corrupt.")));
        }
        let relative = format!("blobs/{}/{}", &hash[..2], hash);
        let target = dir.join(&relative);
        std::fs::create_dir_all(target.parent().unwrap())?;
        std::fs::write(target, bytes)?;
        blobs.insert(relative, hash);
    }
    let manifest =
        Manifest { app_version: env!("CARGO_PKG_VERSION").into(), created_at: time::fmt(state.now()), counts, blobs };
    std::fs::write(dir.join("manifest.json"), serde_json::to_vec_pretty(&manifest)?)?;
    guard.rollback().await?;
    reader.close().await?;
    Ok(json!({"tables":manifest.counts.len(),"blobs":manifest.blobs.len(),"created_at":manifest.created_at}))
}
fn validate_hash(hash: &str) -> AppResult<()> {
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return Err(AppError::internal("Invalid blob hash in backup."));
    }
    Ok(())
}
pub async fn restore_check(state: &AppState, dir: &Path) -> AppResult<()> {
    let id = start(state, "restore_check", dir).await?;
    let scratch = state.cfg.data_dir.join(format!("restore-check-{}-{}", id, rand::random::<u64>()));
    let result = verify(dir, &scratch).await;
    let _ = std::fs::remove_dir_all(&scratch);
    finish(state, id, &result).await?;
    result.map(|_| ())
}
async fn verify(dir: &Path, scratch: &Path) -> AppResult<Value> {
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(dir.join("manifest.json"))?)?;
    std::fs::create_dir_all(scratch)?;
    std::fs::copy(dir.join("servicehub.db"), scratch.join("servicehub.db"))?;
    for (relative, hash) in &manifest.blobs {
        validate_hash(hash)?;
        let expected = format!("blobs/{}/{}", &hash[..2], hash);
        if relative != &expected {
            return Err(AppError::internal("Unsafe blob path in manifest."));
        }
        let bytes = std::fs::read(dir.join(relative))?;
        if hex::encode(Sha256::digest(&bytes)) != *hash {
            return Err(AppError::internal(format!("Blob hash mismatch: {relative}")));
        }
        let target = scratch.join(relative);
        std::fs::create_dir_all(target.parent().unwrap())?;
        std::fs::write(target, bytes)?;
    }
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(scratch.join("servicehub.db")).read_only(true);
    let mut conn = SqliteConnection::connect_with(&options).await?;
    let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check").fetch_all(&mut conn).await?;
    if integrity != vec!["ok"] {
        return Err(AppError::internal(format!("Integrity check: {integrity:?}")));
    }
    if !sqlx::query("PRAGMA foreign_key_check").fetch_all(&mut conn).await?.is_empty() {
        return Err(AppError::internal("Foreign key check failed."));
    }
    if table_counts(&mut conn).await? != manifest.counts {
        return Err(AppError::internal("Table counts differ from the manifest."));
    }
    let hashes: Vec<String> = blob_inventory(&mut conn).await?;
    if hashes.len() != manifest.blobs.len() {
        return Err(AppError::internal("Blob inventory differs from the database."));
    }
    for hash in hashes {
        validate_hash(&hash)?;
        if !manifest.blobs.contains_key(&format!("blobs/{}/{hash}", &hash[..2])) {
            return Err(AppError::internal("A database blob is missing from the manifest."));
        }
    }
    let documents:Vec<(i64,String)>=sqlx::query_as("SELECT d.case_id,b.sha256 FROM documents d JOIN document_versions v ON v.document_id=d.id JOIN blobs b ON b.id=v.blob_id WHERE d.disposed_at IS NULL GROUP BY d.case_id").fetch_all(&mut conn).await?;
    for (_, hash) in &documents {
        std::fs::read(storage::blob_path(&scratch.join("blobs"), hash))?;
    }
    conn.close().await?;
    Ok(
        json!({"integrity_check":"ok","foreign_key_check":"ok","tables":manifest.counts.len(),"blobs":manifest.blobs.len(),"cases_with_readable_documents":documents.len()}),
    )
}
pub async fn daily_job(state: &AppState) -> AppResult<()> {
    let today = time::fmt_date(time::local_date(state.now()));
    let base: PathBuf = state.cfg.data_dir.join("backups").join(&today);
    let prefix = format!("{}/", base.to_string_lossy());
    let existing: Option<String> = sqlx::query_scalar("SELECT location FROM backup_runs WHERE kind='backup' AND status='ok' AND substr(location,1,?)=? ORDER BY id DESC LIMIT 1")
        .bind(prefix.len() as i64).bind(&prefix).fetch_optional(&state.db).await?;
    let result = async {
        let location = match existing {
            Some(path) => PathBuf::from(path),
            None => {
                let path = base.join(format!("run-{}", rand::random::<u64>()));
                backup(state, &path).await?;
                path
            }
        };
        restore_check(state, &location).await
    }
    .await;
    // A failed run must not stop future daily snapshots.
    let mut tx = db::write_tx(&state.db).await?;
    let tomorrow = state.now() + chrono::Duration::days(1);
    crate::jobs::enqueue(
        &mut tx,
        "records.backup",
        json!({}),
        Some(format!("records.backup:{}", time::fmt_date(time::local_date(tomorrow)))),
        tomorrow,
    )
    .await?;
    tx.commit().await?;
    result
}
