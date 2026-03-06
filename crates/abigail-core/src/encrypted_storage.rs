//! File-level encrypted storage using the vault crypto layer.
//!
//! Provides simple encrypt-to-file and decrypt-from-file helpers.
//! Uses AES-256-GCM via the vault envelope format (cross-platform).

use crate::error::Result;
use crate::vault::crypto;
use crate::vault::unlock::HybridUnlockProvider;
use crate::vault::unlock::UnlockProvider;
use std::path::Path;

const STORAGE_SCOPE: &str = "encrypted-storage:general";

/// Write data to a file, encrypting it with AES-256-GCM.
pub fn write_encrypted(path: &Path, data: &[u8]) -> Result<()> {
    let unlock = HybridUnlockProvider::new();
    let root_kek = unlock.root_kek()?;
    write_encrypted_with_kek(path, data, &root_kek)
}

/// Read and decrypt a file encrypted by [`write_encrypted`].
pub fn read_encrypted(path: &Path) -> Result<Vec<u8>> {
    let unlock = HybridUnlockProvider::new();
    let root_kek = unlock.root_kek()?;
    read_encrypted_with_kek(path, &root_kek)
}

fn write_encrypted_with_kek(
    path: &Path,
    data: &[u8],
    root_kek: &[u8; crate::vault::VAULT_KEK_LEN],
) -> Result<()> {
    let dek = crypto::derive_scope_key(&root_kek, STORAGE_SCOPE);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let envelope = crypto::seal(&dek, data)?;
    std::fs::write(path, &envelope)?;
    Ok(())
}

fn read_encrypted_with_kek(
    path: &Path,
    root_kek: &[u8; crate::vault::VAULT_KEK_LEN],
) -> Result<Vec<u8>> {
    let dek = crypto::derive_scope_key(&root_kek, STORAGE_SCOPE);
    let envelope = std::fs::read(path)?;
    crypto::open(&dek, &envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::unlock::PassphraseUnlockProvider;
    use std::fs;

    #[test]
    fn test_write_read_encrypted_roundtrip() {
        let tmp = std::env::temp_dir().join("abigail_enc_storage_v2_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let path = tmp.join("test.enc");
        let data = b"hello encrypted world";
        let unlock = PassphraseUnlockProvider::new("test-enc-storage");
        let root_kek = unlock.root_kek().unwrap();

        write_encrypted_with_kek(&path, data, &root_kek).unwrap();
        assert!(path.exists());

        let decrypted = read_encrypted_with_kek(&path, &root_kek).unwrap();
        assert_eq!(decrypted, data);

        let _ = fs::remove_dir_all(&tmp);
    }
}
