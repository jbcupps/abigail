//! AES-256-GCM envelope encryption and HKDF-based key derivation.
//!
//! All vault data is stored in a versioned envelope:
//!   `[version: u8][nonce: 12B][ciphertext+tag]`
//!
//! The root Key Encryption Key (KEK) is obtained from an `UnlockProvider`.
//! Scope-specific Data Encryption Keys (DEKs) are derived via HKDF-SHA256
//! with a scope label as the `info` parameter.

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::error::{CoreError, Result};

const ENVELOPE_VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// Encrypt `plaintext` with AES-256-GCM using the given 32-byte key.
/// Returns a versioned envelope: `[version][nonce][ciphertext+tag]`.
pub fn seal(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| CoreError::Crypto(format!("AES-GCM seal failed: {}", e)))?;

    let mut envelope = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
    envelope.push(ENVELOPE_VERSION);
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&ciphertext);
    Ok(envelope)
}

/// Decrypt a versioned envelope produced by [`seal`].
pub fn open(key: &[u8; KEY_LEN], envelope: &[u8]) -> Result<Vec<u8>> {
    if envelope.is_empty() {
        return Err(CoreError::Crypto("Empty envelope".into()));
    }
    let version = envelope[0];
    if version != ENVELOPE_VERSION {
        return Err(CoreError::Crypto(format!(
            "Unsupported vault envelope version {}",
            version
        )));
    }
    let min_len = 1 + NONCE_LEN + 16; // version + nonce + AES-GCM tag
    if envelope.len() < min_len {
        return Err(CoreError::Crypto("Envelope too short".into()));
    }
    let nonce = Nonce::from_slice(&envelope[1..1 + NONCE_LEN]);
    let ciphertext = &envelope[1 + NONCE_LEN..];

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    cipher.decrypt(nonce, ciphertext).map_err(|_| {
        CoreError::Crypto("AES-GCM decryption failed (wrong key or tampered data)".into())
    })
}

/// Derive a 32-byte scope key from a root KEK using HKDF-SHA256.
///
/// `scope` is an arbitrary label (e.g. `"hive"`, `"entity:<uuid>"`, `"skills"`).
/// The same root key + scope always produces the same derived key.
pub fn derive_scope_key(root_kek: &[u8; KEY_LEN], scope: &str) -> [u8; KEY_LEN] {
    let hk = Hkdf::<Sha256>::new(None, root_kek);
    let mut okm = [0u8; KEY_LEN];
    hk.expand(scope.as_bytes(), &mut okm)
        .expect("HKDF-SHA256 expand with 32-byte output cannot fail");
    okm
}

/// Check whether `data` looks like a vault envelope (starts with the version byte).
///
/// This is used to distinguish new AES-256-GCM vault files from legacy DPAPI-encrypted files.
pub fn is_vault_envelope(data: &[u8]) -> bool {
    !data.is_empty() && data[0] == ENVELOPE_VERSION
}

/// Derive a 32-byte key from a passphrase using HKDF-SHA256.
///
/// This is intentionally *not* a slow KDF (Argon2/scrypt) because the primary
/// use case is daemon/headless environments where the passphrase comes from an
/// env var or keyfile, not interactive human input. For interactive passphrase
/// entry the OS credential store path should be preferred.
pub fn derive_key_from_passphrase(passphrase: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    let hk = Hkdf::<Sha256>::new(Some(salt), passphrase.as_bytes());
    let mut okm = [0u8; KEY_LEN];
    hk.expand(b"abigail-vault-kek", &mut okm)
        .expect("HKDF-SHA256 expand with 32-byte output cannot fail");
    okm
}

pub fn derive_key_from_passphrase_argon2(
    passphrase: &str,
    salt: &[u8],
    memory_cost_kib: u32,
    time_cost: u32,
    parallelism: u32,
) -> Result<[u8; KEY_LEN]> {
    let params = Params::new(memory_cost_kib, time_cost, parallelism, Some(KEY_LEN))
        .map_err(|e| CoreError::Crypto(format!("Invalid Argon2 parameters: {}", e)))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut okm = [0u8; KEY_LEN];
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut okm)
        .map_err(|e| CoreError::Crypto(format!("Argon2id derivation failed: {}", e)))?;
    Ok(okm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::rand_core::RngCore;

    fn test_key() -> [u8; KEY_LEN] {
        let mut k = [0u8; KEY_LEN];
        k[0] = 0xAB;
        k[31] = 0xCD;
        k
    }

    #[test]
    fn seal_open_roundtrip() {
        let key = test_key();
        let data = b"hello vault world";
        let envelope = seal(&key, data).unwrap();
        assert_eq!(envelope[0], ENVELOPE_VERSION);
        let decrypted = open(&key, &envelope).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn wrong_key_fails() {
        let key = test_key();
        let data = b"secret";
        let envelope = seal(&key, data).unwrap();

        let mut bad_key = key;
        bad_key[0] = 0xFF;
        assert!(open(&bad_key, &envelope).is_err());
    }

    #[test]
    fn tampered_envelope_fails() {
        let key = test_key();
        let mut envelope = seal(&key, b"data").unwrap();
        let last = envelope.len() - 1;
        envelope[last] ^= 0xFF;
        assert!(open(&key, &envelope).is_err());
    }

    #[test]
    fn empty_envelope_fails() {
        let key = test_key();
        assert!(open(&key, &[]).is_err());
    }

    #[test]
    fn scope_key_derivation_deterministic() {
        let root = test_key();
        let k1 = derive_scope_key(&root, "hive");
        let k2 = derive_scope_key(&root, "hive");
        assert_eq!(k1, k2);
    }

    #[test]
    fn different_scopes_produce_different_keys() {
        let root = test_key();
        let k_hive = derive_scope_key(&root, "hive");
        let k_entity = derive_scope_key(&root, "entity:abc-123");
        assert_ne!(k_hive, k_entity);
    }

    fn test_salt() -> [u8; 16] {
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);
        salt
    }

    #[test]
    fn passphrase_derivation_deterministic() {
        let salt = test_salt();
        let k1 = derive_key_from_passphrase("my-passphrase", &salt);
        let k2 = derive_key_from_passphrase("my-passphrase", &salt);
        assert_eq!(k1, k2);
    }

    #[test]
    fn different_passphrases_produce_different_keys() {
        let salt = test_salt();
        let k1 = derive_key_from_passphrase("pass-a", &salt);
        let k2 = derive_key_from_passphrase("pass-b", &salt);
        assert_ne!(k1, k2);
    }

    #[test]
    fn argon2_derivation_is_deterministic() {
        let salt = test_salt();
        let k1 = derive_key_from_passphrase_argon2("my-passphrase", &salt, 64 * 1024, 3, 1)
            .unwrap();
        let k2 = derive_key_from_passphrase_argon2("my-passphrase", &salt, 64 * 1024, 3, 1)
            .unwrap();
        assert_eq!(k1, k2);
    }
}
