//! argon2id password hashing.

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};

use crate::error::{AppError, AppResult};

/// Hashes a password (argon2id, default parameters, random salt) into a PHC string.
pub fn hash(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::internal(format!("password hash: {e}")))
}

/// Verifies a password against a PHC string. Malformed hashes verify as false.
pub fn verify(password: &str, phc: &str) -> bool {
    PasswordHash::new(phc).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
}

/// Burns comparable time when the user does not exist (reduces account enumeration by timing).
pub fn dummy_verify(password: &str) {
    static DUMMY: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| hash("dummy-password-for-timing").unwrap_or_default());
    let _ = verify(password, &DUMMY);
}

#[cfg(test)]
mod tests {
    #[test]
    fn hash_and_verify() {
        let h = super::hash("correct horse battery").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(super::verify("correct horse battery", &h));
        assert!(!super::verify("wrong", &h));
        assert!(!super::verify("x", "not-a-hash"));
    }
}
