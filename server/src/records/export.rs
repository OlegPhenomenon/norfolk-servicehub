//! Audited whole-case exports. ZIP entries are stored without compression.
use super::common::rows;
use crate::{
    auth::Actor,
    authz,
    db::{SqlValue, write_tx},
    error::{AppError, AppResult},
    state::AppState,
    storage, time,
    web::Path,
};
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::json;
pub fn routes() -> Router<AppState> {
    Router::new().route("/api/cases/{id}/export.zip", get(export))
}
async fn export(State(state): State<AppState>, actor: Actor, Path(id): Path<i64>) -> AppResult<Response> {
    let bytes = export_case(&state, &actor, id).await?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/zip"),
            (header::CONTENT_DISPOSITION, "attachment; filename=case-export.zip"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Body::from(bytes),
    )
        .into_response())
}
pub async fn export_case(state: &AppState, actor: &Actor, id: i64) -> AppResult<Vec<u8>> {
    let mut tx = write_tx(&state.db).await?;
    let (case, _) = authz::require_staff_case(&mut tx, actor, id).await?;
    let mut data = json!({"case":case,"exported_at":time::fmt(state.now()),"demo":true});
    for (key, table, column) in [
        ("submission", "submissions", "case_id"),
        ("events", "case_events", "case_id"),
        ("messages", "case_messages", "case_id"),
        ("internal_notes", "internal_notes", "case_id"),
        ("assignments", "case_assignments", "case_id"),
        ("decisions", "decisions", "case_id"),
        ("documents", "documents", "case_id"),
        ("integration_refs", "integration_deliveries", "case_id"),
    ] {
        data[key] = json!(
            rows(&mut tx, &format!("SELECT * FROM {table} WHERE {column}=? ORDER BY id"), &[SqlValue::Int(id)]).await?
        );
    }
    data["decision_evidence"]=json!(rows(&mut tx,"SELECT e.* FROM decision_evidence e JOIN decisions d ON d.id=e.decision_id WHERE d.case_id=? ORDER BY e.decision_id,e.document_version_id",&[SqlValue::Int(id)]).await?);
    // Only links to cases the exporting actor can see are included.
    let scope = authz::case_scope_sql(actor);
    let mut binds = vec![SqlValue::Int(id), SqlValue::Int(id), SqlValue::Int(id)];
    binds.extend(scope.binds);
    data["links"]=json!(rows(&mut tx,&format!("SELECT l.* FROM case_links l JOIN cases c ON c.id=CASE WHEN l.from_case_id=? THEN l.to_case_id ELSE l.from_case_id END WHERE (l.from_case_id=? OR l.to_case_id=?) AND {}",scope.sql),&binds).await?);
    let money: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM invoices WHERE case_id=?)")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    data["money_summary"] = if money {
        json!(crate::finance::api::case_money_summary(&mut tx, id).await?)
    } else {
        json!(crate::finance::api::MoneySummary { settled: true, ..Default::default() })
    };
    let files:Vec<(i64,i64,String,String)>=sqlx::query_as("SELECT d.id,v.version,b.original_name,b.sha256 FROM documents d JOIN document_versions v ON v.document_id=d.id JOIN blobs b ON b.id=v.blob_id WHERE d.case_id=? AND d.disposed_at IS NULL ORDER BY d.id,v.version").bind(id).fetch_all(&mut *tx).await?;
    let mut entries=vec![("case.json".into(),serde_json::to_vec_pretty(&data)?),("README.txt".into(),b"Norfolk ServiceHub case export\nFictional demonstration data.\ncase.json includes frozen submission, events, messages, internal notes, assignments, decisions and evidence version IDs, money summary and external references.\nDocument files are organised by document ID and immutable version. Disposed documents retain metadata only. Times are UTC; dates are Pacific/Norfolk.\nThis package contains confidential material; restrict access.\n".to_vec())];
    for (doc, version, name, hash) in files {
        let name = safe_filename(&name);
        let bytes = std::fs::read(storage::blob_path(&state.cfg.blobs_dir(), &hash))?;
        entries.push((format!("documents/{doc}/{version}-{name}"), bytes));
    }
    let bytes = zip_store(entries)?;
    crate::audit::record(&mut tx, actor.db_id(), "records.export", "case", Some(id), json!({"size_bytes":bytes.len()}))
        .await?;
    crate::cases::core::append_event(
        &mut tx,
        id,
        actor.db_id(),
        "records.export",
        crate::cases::core::Visibility::Staff,
        "Case exported as a ZIP package.",
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(bytes)
}
fn safe_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| if c == '/' || c == '\\' || c == ':' { '_' } else { c })
        .take(180)
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." { "document".into() } else { cleaned }
}
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for b in bytes {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ ((0u32.wrapping_sub(crc & 1)) & 0xedb88320);
        }
    }
    !crc
}
fn u16le(out: &mut Vec<u8>, v: u16) {
    out.extend(v.to_le_bytes());
}
fn u32le(out: &mut Vec<u8>, v: u32) {
    out.extend(v.to_le_bytes());
}
/// Standard ZIP32 stored entries, UTF-8 names, CRC32 and central directory.
fn zip_store(entries: Vec<(String, Vec<u8>)>) -> AppResult<Vec<u8>> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    let count = u16::try_from(entries.len()).map_err(|_| AppError::conflict("Too many files for one export."))?;
    for (name, bytes) in entries {
        let size = u32::try_from(bytes.len()).map_err(|_| AppError::conflict("An export file is too large."))?;
        let offset = u32::try_from(out.len()).map_err(|_| AppError::conflict("The case export is too large."))?;
        let length = u16::try_from(name.len()).map_err(|_| AppError::internal("ZIP filename too long"))?;
        let crc = crc32(&bytes);
        u32le(&mut out, 0x04034b50);
        u16le(&mut out, 20);
        u16le(&mut out, 0x0800);
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u16le(&mut out, 0x0021);
        u32le(&mut out, crc);
        u32le(&mut out, size);
        u32le(&mut out, size);
        u16le(&mut out, length);
        u16le(&mut out, 0);
        out.extend(name.as_bytes());
        out.extend(bytes);
        u32le(&mut directory, 0x02014b50);
        u16le(&mut directory, 20);
        u16le(&mut directory, 20);
        u16le(&mut directory, 0x0800);
        u16le(&mut directory, 0);
        u16le(&mut directory, 0);
        u16le(&mut directory, 0x0021);
        u32le(&mut directory, crc);
        u32le(&mut directory, size);
        u32le(&mut directory, size);
        u16le(&mut directory, length);
        u16le(&mut directory, 0);
        u16le(&mut directory, 0);
        u16le(&mut directory, 0);
        u16le(&mut directory, 0);
        u32le(&mut directory, 0);
        u32le(&mut directory, offset);
        directory.extend(name.as_bytes());
    }
    let start = u32::try_from(out.len()).map_err(|_| AppError::conflict("The case export is too large."))?;
    let size = u32::try_from(directory.len()).map_err(|_| AppError::conflict("The case export is too large."))?;
    out.extend(directory);
    u32le(&mut out, 0x06054b50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, count);
    u16le(&mut out, count);
    u32le(&mut out, size);
    u32le(&mut out, start);
    u16le(&mut out, 0);
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zip_crc_and_safe_names() {
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
        assert_eq!(safe_filename("../../bad\\name.pdf"), ".._.._bad_name.pdf");
        let bytes = zip_store(vec![("a.txt".into(), b"hello".to_vec())]).unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        assert_eq!(&bytes[bytes.len() - 22..bytes.len() - 18], b"PK\x05\x06");
    }
}
