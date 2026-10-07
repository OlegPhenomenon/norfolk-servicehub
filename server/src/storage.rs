//! Content-addressed blob storage with upload validation.
//!
//! Bytes live in `DATA_DIR/blobs/<sha256[0..2]>/<sha256>`; metadata in the `blobs` table.
//!
//! SQLite has one writer, so never touch the pool while holding a `write_tx`:
//! * [`stage`] validates and writes the file (no database access) — call it **before** opening the tx;
//! * [`register`] inserts/reuses the `blobs` row inside the caller's transaction;
//! * [`put`] = stage + register in its own transaction (plain upload endpoints);
//! * [`gc`] deletes unregistered or disposed-only files (never deletes metadata rows).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rand::RngCore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};

use crate::db::write_tx;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::time;

// Serialize staging with GC so a reused hash refreshes its grace period before cleanup.
static FILE_WRITES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Maximum accepted file size (10 MB).
pub const MAX_BYTES: usize = 10 * 1024 * 1024;

/// Which file types an upload slot accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowList {
    /// pdf, png, jpeg, webp — application documents.
    Docs,
    /// csv, json (UTF-8 text) — imports.
    Data,
    /// png, jpeg, webp — photos.
    Image,
}

/// A validated file written to disk but not yet recorded in the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    pub sha256: String,
    pub size_bytes: i64,
    pub mime: String,
    pub original_name: String,
    path: PathBuf,
    bytes: std::sync::Arc<Vec<u8>>,
}

/// A row of the `blobs` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct BlobRow {
    pub id: i64,
    pub sha256: String,
    pub size_bytes: i64,
    pub mime: String,
    pub original_name: String,
    pub scan_status: String,
    pub created_by: Option<i64>,
    pub created_at: String,
}

fn extension(name: &str) -> String {
    Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Detects the MIME type of `bytes` and checks it against the allow-list and the file extension.
pub fn validate(bytes: &[u8], original_name: &str, allow: AllowList) -> AppResult<String> {
    let reject = |msg: &str| AppError::field("file", msg);
    if bytes.is_empty() {
        return Err(reject("The file is empty."));
    }
    if bytes.len() > MAX_BYTES {
        return Err(reject("The file is larger than 10 MB."));
    }
    let ext = extension(original_name);
    let detected = infer::get(bytes).map(|t| t.mime_type());

    // Binary formats by magic bytes.
    let binary = match detected {
        Some("application/pdf") => Some(("application/pdf", &["pdf"][..])),
        Some("image/png") => Some(("image/png", &["png"][..])),
        Some("image/jpeg") => Some(("image/jpeg", &["jpg", "jpeg"][..])),
        Some("image/webp") => Some(("image/webp", &["webp"][..])),
        _ => None,
    };
    if let Some((mime, exts)) = binary {
        let allowed = match allow {
            AllowList::Docs => true,
            AllowList::Image => mime != "application/pdf",
            AllowList::Data => false,
        };
        if !allowed {
            return Err(reject("This type of file is not accepted here."));
        }
        if !exts.contains(&ext.as_str()) {
            return Err(reject("The file's content does not match its extension."));
        }
        return Ok(mime.to_string());
    }
    if detected.is_some() {
        return Err(reject("This type of file is not accepted here."));
    }

    // Text formats (Data only): UTF-8 without control characters.
    if allow != AllowList::Data {
        return Err(reject("This type of file is not accepted here."));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| reject("The file must be UTF-8 text."))?;
    if text.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        return Err(reject("The file contains binary data."));
    }
    match ext.as_str() {
        "csv" => Ok("text/csv".into()),
        "json" => {
            serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}'))
                .map_err(|_| reject("The file is not valid JSON."))?;
            Ok("application/json".into())
        }
        _ => Err(reject("Only .csv and .json files are accepted here.")),
    }
}

/// Path of a blob file.
pub fn blob_path(blobs_dir: &Path, sha256: &str) -> PathBuf {
    blobs_dir.join(&sha256[..2]).join(sha256)
}

/// Validates and writes the file (atomic temp file + rename). No database access.
pub async fn stage(state: &AppState, bytes: &[u8], original_name: &str, allow: AllowList) -> AppResult<Staged> {
    let mime = validate(bytes, original_name, allow)?;
    let _files = FILE_WRITES.lock().await;
    let sha256 = hex::encode(Sha256::digest(bytes));
    let dir = state.cfg.blobs_dir();
    let path = blob_path(&dir, &sha256);
    let data = std::sync::Arc::new(bytes.to_vec());
    let written = data.clone();
    let target = path.clone();
    tokio::task::spawn_blocking(move || write_atomic(&target, &written))
        .await
        .map_err(|e| AppError::internal(format!("blob write task: {e}")))??;
    let name: String = original_name.chars().filter(|c| !c.is_control()).take(200).collect();
    Ok(Staged {
        sha256,
        size_bytes: bytes.len() as i64,
        mime,
        original_name: if name.trim().is_empty() { "file".into() } else { name },
        path,
        bytes: data,
    })
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().expect("blob path has a parent");
    std::fs::create_dir_all(dir)?;
    let mut suffix = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut suffix);
    let tmp = dir.join(format!(".tmp-{}", hex::encode(suffix)));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Records a staged file in `blobs` (dedupe by sha256) inside the caller's transaction.
/// `actor` = `users.id` of the uploader, `None` for system-generated files.
pub async fn register(conn: &mut SqliteConnection, staged: Staged, actor: Option<i64>) -> AppResult<BlobRow> {
    // The caller already holds the DB writer lock. Even if staging waited longer than GC's
    // grace (or GC ran in another process), restore bytes before publishing a new reference.
    let _files = FILE_WRITES.lock().await;
    if !staged.path.exists() {
        let path = staged.path.clone();
        let bytes = staged.bytes.clone();
        tokio::task::spawn_blocking(move || write_atomic(&path, &bytes))
            .await
            .map_err(|e| AppError::internal(format!("blob restore task: {e}")))??;
    }
    sqlx::query(
        "INSERT INTO blobs (sha256, size_bytes, mime, original_name, scan_status, created_by, created_at) \
         VALUES (?, ?, ?, ?, 'not_scanned', ?, ?) ON CONFLICT(sha256) DO NOTHING",
    )
    .bind(&staged.sha256)
    .bind(staged.size_bytes)
    .bind(&staged.mime)
    .bind(&staged.original_name)
    .bind(actor)
    .bind(time::now_str())
    .execute(&mut *conn)
    .await?;
    Ok(sqlx::query_as::<_, BlobRow>("SELECT * FROM blobs WHERE sha256 = ?")
        .bind(&staged.sha256)
        .fetch_one(&mut *conn)
        .await?)
}

/// Stage + register in its own write transaction.
pub async fn put(
    state: &AppState,
    bytes: &[u8],
    original_name: &str,
    allow: AllowList,
    actor: Option<i64>,
) -> AppResult<BlobRow> {
    let staged = stage(state, bytes, original_name, allow).await?;
    let mut tx = write_tx(&state.db).await?;
    let row = register(&mut tx, staged, actor).await?;
    tx.commit().await?;
    Ok(row)
}

/// Reads a blob's metadata and bytes. Callers must check access to whatever references the blob.
pub async fn read(state: &AppState, blob_id: i64) -> AppResult<(BlobRow, Vec<u8>)> {
    let row: BlobRow = sqlx::query_as("SELECT * FROM blobs WHERE id = ?")
        .bind(blob_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(AppError::not_found)?;
    let path = blob_path(&state.cfg.blobs_dir(), &row.sha256);
    let bytes = tokio::fs::read(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound { AppError::not_found() } else { AppError::from(e) }
    })?;
    Ok((row, bytes))
}

/// Registered blobs without a live consumer (including disposed-only documents). Metadata stays intact.
/// Discover blob foreign keys so new slices' consumers are protected automatically.
pub async fn disposed_orphan_hashes(conn: &mut SqliteConnection) -> AppResult<Vec<String>> {
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
            .fetch_all(&mut *conn)
            .await?;
    let mut live = vec![
        "EXISTS(SELECT 1 FROM document_versions v JOIN documents d ON d.id=v.document_id WHERE v.blob_id=b.id AND d.disposed_at IS NULL)".to_string(),
        // Frozen evidence can also be consumed by a different, undisposed case (e.g. review).
        "EXISTS(SELECT 1 FROM document_versions v JOIN submission_documents sd ON sd.document_version_id=v.id JOIN submissions s ON s.id=sd.submission_id JOIN documents source ON source.id=v.document_id WHERE v.blob_id=b.id AND s.case_id<>source.case_id AND NOT EXISTS(SELECT 1 FROM disposal_events e WHERE e.case_id=s.case_id))".to_string(),
        "EXISTS(SELECT 1 FROM document_versions v JOIN decision_evidence e ON e.document_version_id=v.id JOIN decisions d ON d.id=e.decision_id JOIN documents source ON source.id=v.document_id WHERE v.blob_id=b.id AND d.case_id<>source.case_id AND NOT EXISTS(SELECT 1 FROM disposal_events x WHERE x.case_id=d.case_id))".to_string(),
    ];
    for table in tables {
        if table == "document_versions" {
            continue;
        }
        let quoted = table.replace('"', "\"\"");
        for fk in sqlx::query(&format!("PRAGMA foreign_key_list(\"{quoted}\")")).fetch_all(&mut *conn).await? {
            if fk.get::<String, _>("table") == "blobs" {
                let column = fk.get::<String, _>("from").replace('"', "\"\"");
                live.push(format!("EXISTS(SELECT 1 FROM \"{quoted}\" r WHERE r.\"{column}\"=b.id)"));
            }
        }
    }
    let query = format!("SELECT b.sha256 FROM blobs b WHERE NOT ({})", live.join(" OR "));
    Ok(sqlx::query_scalar(&query).fetch_all(conn).await?)
}

/// Deletes unregistered, registered-but-unreferenced and disposed-only document bytes. Files younger than one hour are kept (they may be
/// staged for a transaction that has not committed yet). Returns the number of files removed.
pub async fn gc(state: &AppState) -> AppResult<u64> {
    gc_older_than(state, Duration::from_secs(3600)).await
}

/// [`gc`] with an explicit grace period (`Duration::ZERO` after a full wipe).
pub async fn gc_older_than(state: &AppState, grace: Duration) -> AppResult<u64> {
    // Hold the writer lock through deletion, preventing any new committed reference.
    let mut tx = write_tx(&state.db).await?;
    let _files = FILE_WRITES.lock().await;
    let disposable: std::collections::HashSet<String> = disposed_orphan_hashes(&mut tx).await?.into_iter().collect();
    let known: std::collections::HashSet<String> = sqlx::query_scalar::<_, String>("SELECT sha256 FROM blobs")
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .filter(|hash| !disposable.contains(hash))
        .collect();
    let dir = state.cfg.blobs_dir();
    let removed = tokio::task::spawn_blocking(move || -> std::io::Result<u64> {
        let mut removed = 0;
        let Ok(top) = std::fs::read_dir(&dir) else { return Ok(0) };
        let now = SystemTime::now();
        for sub in top.flatten() {
            if !sub.file_type()?.is_dir() {
                continue;
            }
            for f in std::fs::read_dir(sub.path())?.flatten() {
                let name = f.file_name().to_string_lossy().to_string();
                if known.contains(&name) {
                    continue;
                }
                let age = f.metadata()?.modified().ok().and_then(|m| now.duration_since(m).ok()).unwrap_or_default();
                if age >= grace {
                    std::fs::remove_file(f.path())?;
                    removed += 1;
                }
            }
        }
        Ok(removed)
    })
    .await
    .map_err(|e| AppError::internal(format!("gc task: {e}")))?
    .map_err(AppError::from)?;
    tx.commit().await?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 1, 0, 0, 0, 1, 8,
        6, 0, 0, 0,
    ];

    fn tiny_pdf() -> Vec<u8> {
        b"%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF\n".to_vec()
    }

    #[test]
    fn validation_rules() {
        assert_eq!(validate(PNG, "photo.png", AllowList::Image).unwrap(), "image/png");
        let err = validate(PNG, "plans.pdf", AllowList::Docs).unwrap_err();
        assert!(err.fields["file"].contains("does not match"), "{err:?}");
        assert!(validate(&tiny_pdf(), "x.pdf", AllowList::Image).is_err());
        assert_eq!(validate(b"date,amount\n2026-10-01,100\n", "s.csv", AllowList::Data).unwrap(), "text/csv");
        assert_eq!(validate(br#"{"a":1}"#, "d.json", AllowList::Data).unwrap(), "application/json");
        assert!(validate(b"{not json", "d.json", AllowList::Data).is_err());
        assert!(validate(b"hello", "notes.txt", AllowList::Data).is_err());
        assert!(validate(b"a,b\n", "s.csv", AllowList::Docs).is_err());
        assert!(validate(&[], "e.pdf", AllowList::Docs).is_err());
    }

    #[tokio::test]
    async fn put_dedupes_rejects_and_gc() {
        let (state, _dir) = crate::state::test_support::test_state().await;
        // A PNG disguised as a PDF.
        let err = put(&state, PNG, "drawing.pdf", AllowList::Docs, None).await.unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::Validation);
        // > 10 MB.
        let mut big = tiny_pdf();
        big.resize(MAX_BYTES + 1, b' ');
        let err = put(&state, &big, "big.pdf", AllowList::Docs, None).await.unwrap_err();
        assert!(err.fields["file"].contains("10 MB"));

        let a = put(&state, &tiny_pdf(), "a.pdf", AllowList::Docs, None).await.unwrap();
        let b = put(&state, &tiny_pdf(), "b.pdf", AllowList::Docs, None).await.unwrap();
        assert_eq!(a.id, b.id, "same bytes dedupe to one blob");
        assert_eq!(a.mime, "application/pdf");
        assert_eq!(a.scan_status, "not_scanned");
        let (row, bytes) = read(&state, a.id).await.unwrap();
        assert_eq!((row.id, bytes), (a.id, tiny_pdf()));

        // Orphan files are collected whether or not registration committed.
        let orphan = stage(&state, PNG, "o.png", AllowList::Image).await.unwrap();
        assert_eq!(gc(&state).await.unwrap(), 0, "young files are kept");
        assert_eq!(gc_older_than(&state, Duration::ZERO).await.unwrap(), 2);
        assert!(!blob_path(&state.cfg.blobs_dir(), &orphan.sha256).exists());
        assert!(!blob_path(&state.cfg.blobs_dir(), &a.sha256).exists());
    }
}

#[cfg(test)]
mod orphan_tests {
    use super::*;
    #[tokio::test]
    async fn registered_unreferenced_blob_is_collected_after_one_hour() {
        let (state, _dir) = crate::state::test_support::test_state().await;
        let blob = put(&state, b"%PDF-1.4\nOrphan\n%%EOF", "orphan.pdf", AllowList::Docs, None).await.unwrap();
        assert_eq!(gc(&state).await.unwrap(), 0);
        let path = blob_path(&state.cfg.blobs_dir(), &blob.sha256);
        let old = SystemTime::now() - Duration::from_secs(3601);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(old))
            .unwrap();
        assert_eq!(gc(&state).await.unwrap(), 1);
        assert!(!path.exists());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM blobs WHERE id=?")
                .bind(blob.id)
                .fetch_one(&state.db)
                .await
                .unwrap(),
            1
        );
    }
}
