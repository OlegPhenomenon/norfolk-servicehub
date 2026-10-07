//! Bounded multipart extractor for service form/import uploads. The platform axum dependency does
//! not enable `multipart`; keep this adapter local until that feature can be enabled by its owner.
use crate::{
    error::{AppError, AppResult},
    storage::MAX_BYTES,
};
use axum::{
    body::to_bytes,
    extract::{FromRequest, Request},
};
use std::collections::VecDeque;
pub struct Multipart {
    fields: VecDeque<Part>,
}
pub struct Part {
    name: String,
    filename: Option<String>,
    bytes: Vec<u8>,
}
impl Part {
    pub fn name(&self) -> Option<&str> {
        Some(&self.name)
    }
    pub fn file_name(&self) -> Option<&str> {
        self.filename.as_deref()
    }
    pub async fn bytes(self) -> AppResult<Vec<u8>> {
        Ok(self.bytes)
    }
    pub async fn text(self) -> AppResult<String> {
        String::from_utf8(self.bytes).map_err(|_| AppError::field("file", "Form text must be UTF-8."))
    }
}
impl Multipart {
    pub async fn next_field(&mut self) -> AppResult<Option<Part>> {
        Ok(self.fields.pop_front())
    }
}
impl<S: Send + Sync> FromRequest<S> for Multipart {
    type Rejection = AppError;
    async fn from_request(req: Request, _state: &S) -> AppResult<Self> {
        let content = req.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("");
        if !content.starts_with("multipart/form-data;") {
            return Err(AppError::field("file", "Upload a multipart form."));
        }
        let boundary = content
            .split(';')
            .map(str::trim)
            .find_map(|s| s.strip_prefix("boundary="))
            .map(|s| s.trim_matches('"'))
            .filter(|b| !b.is_empty() && b.len() <= 70 && b.bytes().all(|c| c.is_ascii_graphic()))
            .ok_or_else(|| AppError::field("file", "Invalid multipart boundary."))?
            .to_owned();
        let bytes = to_bytes(req.into_body(), MAX_BYTES + 64 * 1024)
            .await
            .map_err(|_| AppError::field("file", "The upload is larger than 10 MB."))?;
        parse(&bytes, &boundary)
    }
}
fn position(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes.windows(needle.len()).position(|w| w == needle)
}
fn parameter(header: &str, key: &str) -> Option<String> {
    header.split(';').skip(1).map(str::trim).find_map(|p| {
        let (k, v) = p.split_once('=')?;
        if k == key {
            Some(v.trim_matches('"').replace("%22", "\"").replace("%0D", "").replace("%0A", ""))
        } else {
            None
        }
    })
}
fn parse(bytes: &[u8], boundary: &str) -> AppResult<Multipart> {
    let reject = || AppError::field("file", "Malformed multipart upload.");
    let opening = format!("--{boundary}\r\n");
    let delimiter = format!("\r\n--{boundary}");
    if !bytes.starts_with(opening.as_bytes()) {
        return Err(reject());
    }
    let mut offset = opening.len();
    let mut fields = VecDeque::new();
    loop {
        let head_end = position(&bytes[offset..], b"\r\n\r\n").filter(|n| *n <= 8192).ok_or_else(reject)?;
        let headers = std::str::from_utf8(&bytes[offset..offset + head_end]).map_err(|_| reject())?;
        let disposition = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-disposition").then_some(value.trim())
            })
            .filter(|s| s.starts_with("form-data;"))
            .ok_or_else(reject)?;
        let name = parameter(disposition, "name").ok_or_else(reject)?;
        let filename = parameter(disposition, "filename");
        let start = offset + head_end + 4;
        // Only a boundary followed by CRLF or '--' terminates a binary part.
        let mut scan = start;
        let end = loop {
            let candidate = scan + position(&bytes[scan..], delimiter.as_bytes()).ok_or_else(reject)?;
            let after = candidate + delimiter.len();
            if bytes.get(after..after + 2).is_some_and(|s| s == b"\r\n" || s == b"--") {
                break candidate;
            }
            scan = after;
        };
        if end - start > MAX_BYTES || fields.len() >= 32 {
            return Err(AppError::field("file", "Upload one file and at most 31 form fields, up to 10 MB."));
        }
        fields.push_back(Part { name, filename, bytes: bytes[start..end].to_vec() });
        offset = end + delimiter.len();
        if bytes.get(offset..offset + 2) == Some(b"--") {
            break;
        }
        offset += 2;
    }
    Ok(Multipart { fields })
}
#[cfg(test)]
mod tests {
    #[test]
    fn binary_parts_and_lookalike_boundaries() {
        let bytes=b"--b\r\nContent-Disposition: form-data; name=\"slug\"\r\n\r\na\r\n--b\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.pdf\"\r\nContent-Type: application/pdf\r\n\r\n%PDF\xff\r\n--bX\r\n--b--\r\n";
        let mut form = super::parse(bytes, "b").unwrap();
        assert_eq!(form.fields.pop_front().unwrap().bytes, b"a");
        let file = form.fields.pop_front().unwrap();
        assert_eq!(file.filename.as_deref(), Some("a.pdf"));
        assert_eq!(file.bytes, b"%PDF\xff\r\n--bX");
        assert!(super::parse(b"bad", "b").is_err());
    }
}
