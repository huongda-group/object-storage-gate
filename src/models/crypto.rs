use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use loco_rs::prelude::*;
use std::sync::OnceLock;

pub const NONCE_LEN: usize = 12;

// ponytail: dev/test fallback key.
// Production MUST set OSG_MASTER_KEY to a base64-encoded 32-byte key — enforced in `app::App::after_context`, which refuses to start a production app whose key is missing, malformed, or equal to this one, so this fallback can only be reached in development and test.
// Upgrade path: KMS-backed key if rotation needed.
pub const DEV_KEY_B64: &str = "ZGV2LW9ubHktMzJieXRlLW1hc3Rlci1rZXktMDEyMzQ=";

/// Checks a candidate master key before the process commits to it.
///
/// Called at boot rather than at first use: `master_key()` caches in a `OnceLock` and panics on a bad key, and a panic at the first key creation in production is a much worse failure than a refused boot.
///
/// # Errors
///
/// Returns an error when the value is not valid base64, does not decode to exactly 32 bytes, or is the development key committed to this repository.
pub fn validate_master_key(b64: &str) -> Result<()> {
    let trimmed = b64.trim();
    if trimmed == DEV_KEY_B64 {
        return Err(Error::string(
            "OSG_MASTER_KEY is the development key committed to this repository; generate a new one with `openssl rand -base64 32`",
        ));
    }
    let bytes = STANDARD
        .decode(trimmed)
        .map_err(|_| Error::string("OSG_MASTER_KEY must be valid base64"))?;
    if bytes.len() != 32 {
        return Err(Error::string(
            "OSG_MASTER_KEY must decode to exactly 32 bytes",
        ));
    }
    Ok(())
}

fn master_key() -> &'static Key<Aes256Gcm> {
    static KEY: OnceLock<Key<Aes256Gcm>> = OnceLock::new();
    KEY.get_or_init(|| {
        let b64 = std::env::var("OSG_MASTER_KEY").unwrap_or_else(|_| DEV_KEY_B64.to_string());
        let bytes = STANDARD
            .decode(b64.trim())
            .expect("OSG_MASTER_KEY must be valid base64");
        assert_eq!(bytes.len(), 32, "OSG_MASTER_KEY must decode to 32 bytes");
        *Key::<Aes256Gcm>::from_slice(&bytes)
    })
}

/// Encrypt a secret for storage. Layout: `nonce || ciphertext || tag`.
///
/// # Panics
///
/// Panics if `OSG_MASTER_KEY` does not decode to 32 bytes, or if AES-GCM encryption fails — both are deployment faults, not bad runtime input.
#[must_use]
pub fn encrypt(plaintext: &str) -> Vec<u8> {
    let cipher = Aes256Gcm::new(master_key());
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let mut ct = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .expect("encrypt");
    let mut out = nonce.to_vec();
    out.append(&mut ct);
    out
}

/// Decrypt a stored secret. Fails on truncated/tampered input.
///
/// # Errors
/// Returns an error if input is too short or authentication fails.
pub fn decrypt(data: &[u8]) -> Result<String> {
    if data.len() <= NONCE_LEN {
        return Err(Error::string("ciphertext too short"));
    }
    let (nonce_bytes, ct) = data.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(master_key());
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ct)
        .map_err(|_| Error::string("decrypt failed"))?;
    String::from_utf8(pt).map_err(|e| Error::string(&e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = "s3cr3t-access-key-value";
        let blob = encrypt(s);
        assert_ne!(blob, s.as_bytes());
        assert_eq!(decrypt(&blob).unwrap(), s);
    }
    #[test]
    fn nonce_is_random() {
        assert_ne!(encrypt("same"), encrypt("same"));
    }
    #[test]
    fn tampered_fails() {
        let mut blob = encrypt("secret");
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        assert!(decrypt(&blob).is_err());
    }
    #[test]
    fn too_short_fails() {
        assert!(decrypt(b"short").is_err());
    }

    #[test]
    fn validate_rejects_the_dev_key() {
        assert!(validate_master_key(DEV_KEY_B64).is_err());
    }

    #[test]
    fn validate_rejects_bad_base64_and_wrong_length() {
        assert!(validate_master_key("not base64!!").is_err());
        assert!(validate_master_key("").is_err());
        // 31 bytes, one short.
        assert!(validate_master_key(&STANDARD.encode([7u8; 31])).is_err());
    }

    #[test]
    fn validate_accepts_a_real_key() {
        assert!(validate_master_key(&STANDARD.encode([7u8; 32])).is_ok());
    }
}
