//! Vault abstraction for external public key storage.
//!
//! The external vault stores the **public key** used to verify constitutional documents.
//! Abby can **read** from this vault but cannot write to it. The private signing key
//! is created and managed out-of-band (e.g., GPG, OpenSSL) and never stored in Abby.

use crate::error::{CoreError, Result};
use base64::Engine as _;
use ed25519_dalek::VerifyingKey;
use std::path::Path;

/// Trait for external vaults that provide read-only access to the signing public key.
pub trait ExternalVault: Send + Sync {
    /// Read the public key from the vault. Returns an error if unreadable.
    fn read_public_key(&self) -> Result<VerifyingKey>;
}

/// Read-only file-based vault for MVP/testing.
///
/// Expects the public key file to contain raw 32-byte Ed25519 public key,
/// or a PEM-encoded public key (auto-detected).
pub struct ReadOnlyFileVault {
    path: std::path::PathBuf,
}

impl ReadOnlyFileVault {
    /// Create a new file vault pointing to the given path.
    /// The path should be outside Abby's data directory and protected by OS ACLs.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Check if the vault file exists and is readable.
    pub fn exists(&self) -> bool {
        self.path.exists()
    }
}

impl ExternalVault for ReadOnlyFileVault {
    fn read_public_key(&self) -> Result<VerifyingKey> {
        if !self.path.exists() {
            return Err(CoreError::Vault(format!(
                "External public key not found: {}",
                self.path.display()
            )));
        }

        let data = std::fs::read(&self.path).map_err(|e| {
            CoreError::Vault(format!(
                "Failed to read external public key at {}: {}",
                self.path.display(),
                e
            ))
        })?;

        // Try raw 32-byte key first
        if data.len() == 32 {
            let bytes: [u8; 32] = data
                .try_into()
                .map_err(|_| CoreError::Vault("Invalid key length".into()))?;
            return VerifyingKey::from_bytes(&bytes)
                .map_err(|e| CoreError::Vault(format!("Invalid Ed25519 public key: {}", e)));
        }

        // Try PEM format (-----BEGIN PUBLIC KEY-----)
        if let Ok(text) = std::str::from_utf8(&data) {
            if text.contains("-----BEGIN") {
                return parse_pem_public_key(text);
            }
        }

        // Try base64-encoded raw key
        if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(&data) {
            if decoded.len() == 32 {
                let bytes: [u8; 32] = decoded
                    .try_into()
                    .map_err(|_| CoreError::Vault("Invalid key length".into()))?;
                return VerifyingKey::from_bytes(&bytes)
                    .map_err(|e| CoreError::Vault(format!("Invalid Ed25519 public key: {}", e)));
            }
        }

        Err(CoreError::Vault(format!(
            "Unrecognized public key format at {}. Expected 32-byte raw, base64, or PEM.",
            self.path.display()
        )))
    }
}

/// Parse a PEM-encoded Ed25519 public key.
/// Supports PKCS#8 SubjectPublicKeyInfo format.
fn parse_pem_public_key(pem: &str) -> Result<VerifyingKey> {
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine as _;

    // Extract base64 content between BEGIN and END markers
    let lines: Vec<&str> = pem
        .lines()
        .filter(|l| !l.starts_with("-----") && !l.is_empty())
        .collect();
    let b64 = lines.join("");

    let der = BASE64
        .decode(&b64)
        .map_err(|e| CoreError::Vault(format!("Invalid PEM base64: {}", e)))?;

    // Ed25519 PKCS#8 public key DER has a fixed prefix (12 bytes) before the 32-byte key
    // OID 1.3.101.112 (Ed25519) in SubjectPublicKeyInfo
    const ED25519_SPKI_PREFIX: [u8; 12] = [
        0x30, 0x2a, // SEQUENCE, 42 bytes
        0x30, 0x05, // SEQUENCE, 5 bytes (AlgorithmIdentifier)
        0x06, 0x03, 0x2b, 0x65, 0x70, // OID 1.3.101.112 (Ed25519)
        0x03, 0x21, 0x00, // BIT STRING, 33 bytes (0x00 padding + 32-byte key)
    ];

    if der.len() == 44 && der.starts_with(&ED25519_SPKI_PREFIX) {
        let key_bytes: [u8; 32] = der[12..]
            .try_into()
            .map_err(|_| CoreError::Vault("Invalid key length in SPKI".into()))?;
        return VerifyingKey::from_bytes(&key_bytes)
            .map_err(|e| CoreError::Vault(format!("Invalid Ed25519 public key: {}", e)));
    }

    // Fallback: if DER is exactly 32 bytes, treat as raw key
    if der.len() == 32 {
        let key_bytes: [u8; 32] = der
            .try_into()
            .map_err(|_| CoreError::Vault("Invalid key length".into()))?;
        return VerifyingKey::from_bytes(&key_bytes)
            .map_err(|e| CoreError::Vault(format!("Invalid Ed25519 public key: {}", e)));
    }

    Err(CoreError::Vault(format!(
        "Unsupported PEM key format (DER length: {}). Expected Ed25519 SPKI (44 bytes) or raw (32 bytes).",
        der.len()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use std::fs;

    #[test]
    fn test_read_raw_public_key() {
        let temp = std::env::temp_dir().join("abby_vault_test_raw");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let signing = SigningKey::generate(&mut OsRng);
        let pubkey = signing.verifying_key();
        let pubkey_path = temp.join("pubkey.bin");
        fs::write(&pubkey_path, pubkey.to_bytes()).unwrap();

        let vault = ReadOnlyFileVault::new(&pubkey_path);
        let loaded = vault.read_public_key().unwrap();
        assert_eq!(loaded.to_bytes(), pubkey.to_bytes());

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_read_base64_public_key() {
        use base64::engine::general_purpose::STANDARD as BASE64;
        use base64::Engine as _;

        let temp = std::env::temp_dir().join("abby_vault_test_b64");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let signing = SigningKey::generate(&mut OsRng);
        let pubkey = signing.verifying_key();
        let pubkey_path = temp.join("pubkey.b64");
        fs::write(&pubkey_path, BASE64.encode(pubkey.to_bytes())).unwrap();

        let vault = ReadOnlyFileVault::new(&pubkey_path);
        let loaded = vault.read_public_key().unwrap();
        assert_eq!(loaded.to_bytes(), pubkey.to_bytes());

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_missing_vault_file() {
        let vault = ReadOnlyFileVault::new("/nonexistent/path/pubkey.bin");
        let result = vault.read_public_key();
        assert!(result.is_err());
    }
}
