//! File-level encrypted storage using DPAPI (Windows) or plaintext (dev).
//!
//! Wraps `dpapi_encrypt`/`dpapi_decrypt` for reading and writing encrypted files.

use crate::dpapi::{dpapi_decrypt, dpapi_encrypt};
use crate::error::Result;
use std::path::Path;

/// Write data to a file, encrypting it with DPAPI.
pub fn write_encrypted(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let encrypted = dpapi_encrypt(data)?;
    std::fs::write(path, &encrypted)?;
    Ok(())
}

/// Read and decrypt a DPAPI-encrypted file.
pub fn read_encrypted(path: &Path) -> Result<Vec<u8>> {
    let encrypted = std::fs::read(path)?;
    dpapi_decrypt(&encrypted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_write_read_encrypted_roundtrip() {
        let tmp = std::env::temp_dir().join("abigail_enc_storage_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let path = tmp.join("test.enc");
        let data = b"hello encrypted world";

        write_encrypted(&path, data).unwrap();
        assert!(path.exists());

        let decrypted = read_encrypted(&path).unwrap();
        assert_eq!(decrypted, data);

        let _ = fs::remove_dir_all(&tmp);
    }
}
