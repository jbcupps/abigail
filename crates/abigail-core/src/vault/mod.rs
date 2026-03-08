//! Vault subsystem: cross-platform encrypted secrets storage.
//!
//! This module provides:
//! - `crypto`   — AES-256-GCM envelope encrypt/decrypt + HKDF key derivation
//! - `unlock`   — hybrid unlock providers (OS credential store / passphrase / DPAPI)
//! - `scoped`   — Hive/entity/skills scoped vault built on the crypto layer
//! - `external` — read-only external pubkey vault (document signing, legacy)

use crate::error::{CoreError, Result};
use crate::secure_fs;
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub mod crypto;
pub mod external;
pub mod scoped;
pub mod unlock;

pub use external::{ExternalVault, ReadOnlyFileVault};
pub use scoped::{ScopedVault, VaultScope};
pub use unlock::UnlockProvider;

pub(crate) const VAULT_KEK_LEN: usize = 32;
pub(crate) const SENTINEL_FILE_NAME: &str = "vault.sentinel";
pub(crate) const SENTINEL_SCOPE: &str = "vault:sentinel:v1";
pub(crate) const SENTINEL_PREFIX: &str = "ABIGAIL_VAULT_v1_";

static SESSION_ROOT_KEK: OnceLock<[u8; VAULT_KEK_LEN]> = OnceLock::new();
static SESSION_SENTINEL_VALUE: OnceLock<String> = OnceLock::new();
static SESSION_VERIFIED_AT_UTC: OnceLock<String> = OnceLock::new();

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VaultRuntimeStatus {
    pub healthy: bool,
    pub verified_at_utc: String,
    pub sentinel_value: String,
    pub sentinel_timestamp_utc: Option<String>,
}

pub fn current_runtime_status() -> Option<VaultRuntimeStatus> {
    let sentinel = SESSION_SENTINEL_VALUE.get()?.to_string();
    let verified_at_utc = SESSION_VERIFIED_AT_UTC.get()?.to_string();
    Some(VaultRuntimeStatus {
        healthy: true,
        verified_at_utc,
        sentinel_timestamp_utc: sentinel_timestamp_utc(&sentinel),
        sentinel_value: sentinel,
    })
}

pub(crate) fn cached_session_root_kek() -> Option<[u8; VAULT_KEK_LEN]> {
    SESSION_ROOT_KEK.get().copied()
}

pub(crate) fn cache_session_root_kek(root_kek: [u8; VAULT_KEK_LEN], sentinel_value: String) {
    let _ = SESSION_ROOT_KEK.set(root_kek);
    let _ = SESSION_SENTINEL_VALUE.set(sentinel_value);
    let _ = SESSION_VERIFIED_AT_UTC.set(Utc::now().to_rfc3339());
}

pub(crate) fn sentinel_path(data_root: &Path) -> PathBuf {
    data_root.join(SENTINEL_FILE_NAME)
}

pub(crate) fn write_encrypted_sentinel(
    data_root: &Path,
    root_kek: &[u8; VAULT_KEK_LEN],
) -> Result<String> {
    std::fs::create_dir_all(data_root)?;
    let sentinel_value = format!("{}{}", SENTINEL_PREFIX, Utc::now().to_rfc3339());
    let dek = crypto::derive_scope_key(root_kek, SENTINEL_SCOPE);
    let envelope = crypto::seal(&dek, sentinel_value.as_bytes())?;
    secure_fs::write_bytes_atomic(&sentinel_path(data_root), &envelope)?;
    Ok(sentinel_value)
}

pub(crate) fn decrypt_sentinel(data_root: &Path, root_kek: &[u8; VAULT_KEK_LEN]) -> Result<String> {
    let bytes = std::fs::read(sentinel_path(data_root))?;
    decrypt_sentinel_bytes(&bytes, root_kek)
}

pub(crate) fn decrypt_sentinel_bytes(
    bytes: &[u8],
    root_kek: &[u8; VAULT_KEK_LEN],
) -> Result<String> {
    let dek = crypto::derive_scope_key(root_kek, SENTINEL_SCOPE);
    let plaintext = crypto::open(&dek, bytes)?;
    let sentinel = String::from_utf8(plaintext)
        .map_err(|e| CoreError::Vault(format!("Invalid sentinel UTF-8 payload: {}", e)))?;
    validate_sentinel(&sentinel)?;
    Ok(sentinel)
}

pub(crate) fn sentinel_timestamp_utc(sentinel: &str) -> Option<String> {
    sentinel
        .strip_prefix(SENTINEL_PREFIX)
        .map(|ts| ts.to_string())
}

fn validate_sentinel(sentinel: &str) -> Result<()> {
    if sentinel.starts_with(SENTINEL_PREFIX) {
        Ok(())
    } else {
        Err(CoreError::Vault(format!(
            "Invalid sentinel payload. Expected '{}' prefix.",
            SENTINEL_PREFIX
        )))
    }
}

/// Initialize vault/bootstrap logic on a blocking worker and enforce
/// runtime verification without mutating identity data on failure.
pub async fn init_resilient() {
    let result = tokio::task::spawn_blocking(|| {
        let data_root = crate::AppConfig::default_paths().data_dir;
        let _ = std::fs::create_dir_all(&data_root);

        if let Err(e) = crate::SecretsVault::load(data_root.clone()) {
            // paper Sections 22-27 runtime verification:
            // never mutate identity/vault data during boot verification failure.
            tracing::error!("Vault runtime verification failed: {}", e);
        }
    })
    .await;

    if let Err(e) = result {
        tracing::error!("Resilient vault bootstrap task failed to join: {}", e);
    }
}
