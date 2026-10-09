//! Password / API-key hashing (argon2id) and random tokens.
//!
//! * Passwords and webhook API keys are stored as argon2id PHC strings (salted, memory-hard).
//! * Refresh tokens are 256-bit random values; only their SHA-256 is stored (fast lookup, and a
//!   DB leak does not leak usable tokens).
//! * [`dummy_verify`] burns the same CPU as a real verification so that "unknown email" and
//!   "wrong password" take the same time (no user enumeration through timing).

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use once_cell::sync::Lazy;
use platform::error::{AppError, AppResult};
use rand::RngCore;
use sha2::{Digest, Sha256};

pub fn hash_secret(secret: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(secret.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::internal(format!("argon2 hash: {e}")))
}

pub fn verify_secret(secret: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(parsed) => Argon2::default()
            .verify_password(secret.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

static DUMMY_HASH: Lazy<String> = Lazy::new(|| hash_secret("dummy-password-for-timing").unwrap_or_default());

/// Constant-time failure path for unknown users.
pub fn dummy_verify(secret: &str) {
    let _ = verify_secret(secret, &DUMMY_HASH);
}

/// URL-safe random token with `bytes` bytes of entropy.
pub fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

pub fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// Length of the stored lookup prefix of a webhook API key.
pub const API_KEY_PREFIX_LEN: usize = 12;

/// New webhook API key: `fdk_` + 40 random chars. Returns `(key, prefix)`.
pub fn new_api_key() -> (String, String) {
    let key = format!("fdk_{}", random_token(30));
    let prefix = key.chars().take(API_KEY_PREFIX_LEN).collect();
    (key, prefix)
}

pub fn api_key_prefix(key: &str) -> Option<String> {
    if key.starts_with("fdk_") && key.len() > API_KEY_PREFIX_LEN {
        Some(key.chars().take(API_KEY_PREFIX_LEN).collect())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let h = hash_secret("s3cret").unwrap_or_default();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_secret("s3cret", &h));
        assert!(!verify_secret("wrong", &h));
        assert!(!verify_secret("s3cret", "not-a-hash"));
    }

    #[test]
    fn api_keys() {
        let (k, p) = new_api_key();
        assert!(k.starts_with("fdk_"));
        assert_eq!(p.len(), API_KEY_PREFIX_LEN);
        assert_eq!(api_key_prefix(&k).as_deref(), Some(p.as_str()));
        assert_eq!(api_key_prefix("nope"), None);
        assert_ne!(new_api_key().0, k);
        assert_eq!(sha256_hex("a").len(), 64);
    }
}
