//! TOTP (RFC 6238): SHA-1, 6 digits, 30 s steps, ±1 step tolerance. Replays are rejected by
//! remembering the last accepted step in `users.totp_last_step`.

use rand::RngCore;
use totp_rs::{Algorithm, Secret, TOTP};

use crate::error::{AppError, AppResult};

pub const STEP_SECS: u64 = 30;
pub const DIGITS: usize = 6;
pub const ISSUER: &str = "Norfolk ServiceHub";

/// A new random 160-bit secret, base32 (RFC 4648, no padding).
pub fn generate_secret() -> String {
    let mut bytes = [0u8; 20];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    Secret::Raw(bytes.to_vec()).to_encoded().to_string()
}

fn totp(secret_b32: &str) -> AppResult<TOTP> {
    let bytes = Secret::Encoded(secret_b32.to_string())
        .to_bytes()
        .map_err(|e| AppError::internal(format!("invalid TOTP secret: {e:?}")))?;
    TOTP::new(Algorithm::SHA1, DIGITS, 1, STEP_SECS, bytes).map_err(|e| AppError::internal(format!("TOTP: {e:?}")))
}

/// Time step of a unix time.
pub fn step_of(unix_secs: u64) -> u64 {
    unix_secs / STEP_SECS
}

/// The code for `unix_secs` (used by the demo authenticator).
pub fn code_at(secret_b32: &str, unix_secs: u64) -> AppResult<String> {
    Ok(totp(secret_b32)?.generate(unix_secs))
}

/// Seconds until the current code changes.
pub fn seconds_left(unix_secs: u64) -> u64 {
    STEP_SECS - unix_secs % STEP_SECS
}

/// Returns the matching time step if `code` is valid at `unix_secs` ± one step.
/// The caller must reject the step if it is `<= users.totp_last_step` (replay).
pub fn matching_step(secret_b32: &str, code: &str, unix_secs: u64) -> AppResult<Option<u64>> {
    let code = code.trim().replace(' ', "");
    if code.len() != DIGITS || !code.chars().all(|c| c.is_ascii_digit()) {
        return Ok(None);
    }
    let t = totp(secret_b32)?;
    let now_step = step_of(unix_secs);
    // Prefer the newest matching step.
    for step in [now_step + 1, now_step, now_step.saturating_sub(1)] {
        if crate::web::ct_eq(&t.generate(step * STEP_SECS), &code) {
            return Ok(Some(step));
        }
    }
    Ok(None)
}

/// `otpauth://` URL for authenticator apps.
pub fn otpauth_url(secret_b32: &str, account: &str) -> String {
    fn enc(s: &str) -> String {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'@' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect()
    }
    format!(
        "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits={}&period={}",
        enc(ISSUER),
        enc(account),
        secret_b32,
        enc(ISSUER),
        DIGITS,
        STEP_SECS
    )
}

/// QR code (SVG markup) for an otpauth URL.
pub fn qr_svg(url: &str) -> AppResult<String> {
    let code = qrcode::QrCode::new(url.as_bytes()).map_err(|e| AppError::internal(format!("QR: {e}")))?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .dark_color(qrcode::render::svg::Color("#0b2545"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_match_within_one_step() {
        let s = generate_secret();
        let t = 1_800_000_000u64;
        let code = code_at(&s, t).unwrap();
        assert_eq!(matching_step(&s, &code, t).unwrap(), Some(step_of(t)));
        assert_eq!(matching_step(&s, &code, t + 30).unwrap(), Some(step_of(t)));
        assert_eq!(matching_step(&s, &code, t - 30).unwrap(), Some(step_of(t)));
        assert_eq!(matching_step(&s, &code, t + 90).unwrap(), None);
        assert_eq!(matching_step(&s, "12x456", t).unwrap(), None);
        assert!(
            otpauth_url(&s, "olga@demo.servicehub.invalid").starts_with("otpauth://totp/Norfolk%20ServiceHub:olga@")
        );
        assert!(qr_svg("otpauth://totp/x").unwrap().contains("<svg"));
    }
}
