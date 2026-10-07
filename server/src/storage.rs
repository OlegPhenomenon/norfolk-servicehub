//! Content-addressed blob storage with upload validation.
//!
//! Bytes live in `DATA_DIR/blobs/<sha256[0..2]>/<sha256>`; metadata in the `blobs` table.
//!
//! SQLite has one writer, so never touch the pool while holding a `write_tx`:
//! * [`stage`] validates and writes the file (no database access) — call it **before** opening the tx;
//! * [`register`] inserts/reuses the `blobs` row inside the caller's transaction;
//! * [`put`] = stage + register in its own transaction (plain upload endpoints);
//! * [`gc`] deletes files no `blobs` row references (never deletes rows).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rand::RngCore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

use crate::db::write_tx;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::time;

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
    let sha256 = hex::encode(Sha256::digest(bytes));
    let dir = state.cfg.blobs_dir();
    let path = blob_path(&dir, &sha256);
    let data = bytes.to_vec();
    let target = path.clone();
    tokio::task::spawn_blocking(move || write_atomic(&target, &data))
        .await
        .map_err(|e| AppError::internal(format!("blob write task: {e}")))??;
    let name: String = original_name.chars().filter(|c| !c.is_control()).take(200).collect();
    Ok(Staged {
        sha256,
        size_bytes: bytes.len() as i64,
        mime,
        original_name: if name.trim().is_empty() { "file".into() } else { name },
    })
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if path.exists() {
        return Ok(()); // content-addressed: identical bytes already stored
    }
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
    sqlx::query(
        "INSERT INTO blobs (sha256, size_bytes, mime, original_name, scan_status, created_by, created_at) \
         VALUES (?, ?, ?, ?, 'clean', ?, ?) ON CONFLICT(sha256) DO NOTHING",
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

/// Deletes blob files without a `blobs` row. Files younger than 10 minutes are kept (they may be
/// staged for a transaction that has not committed yet). Returns the number of files removed.
pub async fn gc(state: &AppState) -> AppResult<u64> {
    gc_older_than(state, Duration::from_secs(600)).await
}

/// [`gc`] with an explicit grace period (`Duration::ZERO` after a full wipe).
pub async fn gc_older_than(state: &AppState, grace: Duration) -> AppResult<u64> {
    let known: std::collections::HashSet<String> =
        sqlx::query_scalar::<_, String>("SELECT sha256 FROM blobs").fetch_all(&state.db).await?.into_iter().collect();
    let dir = state.cfg.blobs_dir();
    tokio::task::spawn_blocking(move || -> std::io::Result<u64> {
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
    .map_err(AppError::from)
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
        assert_eq!(a.scan_status, "clean");
        let (row, bytes) = read(&state, a.id).await.unwrap();
        assert_eq!((row.id, bytes), (a.id, tiny_pdf()));

        // An orphan file (staged, never registered) is removed by gc; registered ones stay.
        let orphan = stage(&state, PNG, "o.png", AllowList::Image).await.unwrap();
        assert_eq!(gc(&state).await.unwrap(), 0, "young files are kept");
        assert_eq!(gc_older_than(&state, Duration::ZERO).await.unwrap(), 1);
        assert!(!blob_path(&state.cfg.blobs_dir(), &orphan.sha256).exists());
        assert!(blob_path(&state.cfg.blobs_dir(), &a.sha256).exists());
    }
}
