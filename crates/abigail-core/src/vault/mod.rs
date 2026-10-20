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
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

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

type CacheKey = (PathBuf, Option<String>);
struct SessionKey {
    root_kek: [u8; VAULT_KEK_LEN],
    status: VaultRuntimeStatus,
}
static SESSION_KEYS: OnceLock<Mutex<HashMap<CacheKey, SessionKey>>> = OnceLock::new();

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VaultRuntimeStatus {
    pub healthy: bool,
    pub verified_at_utc: String,
    pub sentinel_value: String,
    pub sentinel_timestamp_utc: Option<String>,
}

pub fn current_runtime_status() -> Option<VaultRuntimeStatus> {
    let context = unlock::CredentialContext::current().ok()?;
    let cache = SESSION_KEYS.get()?.lock().ok()?;
    Some(cache.get(&context.cache_key())?.status.clone())
}

pub(crate) fn cached_session_root_kek(key: &CacheKey) -> Option<[u8; VAULT_KEK_LEN]> {
    Some(SESSION_KEYS.get()?.lock().ok()?.get(key)?.root_kek)
}

pub(crate) fn cache_session_root_kek(
    key: CacheKey,
    root_kek: [u8; VAULT_KEK_LEN],
    sentinel_value: String,
) {
    if let Ok(mut cache) = SESSION_KEYS.get_or_init(Default::default).lock() {
        cache.insert(
            key,
            SessionKey {
                root_kek,
                status: VaultRuntimeStatus {
                    healthy: true,
                    verified_at_utc: Utc::now().to_rfc3339(),
                    sentinel_timestamp_utc: sentinel_timestamp_utc(&sentinel_value),
                    sentinel_value,
                },
            },
        );
    }
}

pub(crate) fn sentinel_path(data_root: &Path) -> PathBuf {
    data_root.join(SENTINEL_FILE_NAME)
}

pub(crate) fn write_encrypted_sentinel(
    data_root: &Path,
    root_kek: &[u8; VAULT_KEK_LEN],
) -> Result<String> {
    unlock::validate_credential_path(&sentinel_path(data_root))?;
    write_encrypted_sentinel_with_scope(
        data_root,
        root_kek,
        &unlock::credential_scope_label(SENTINEL_SCOPE)?,
    )
}

pub(crate) fn write_encrypted_sentinel_with_scope(
    data_root: &Path,
    root_kek: &[u8; VAULT_KEK_LEN],
    scope: &str,
) -> Result<String> {
    std::fs::create_dir_all(data_root)?;
    let sentinel_value = format!("{}{}", SENTINEL_PREFIX, Utc::now().to_rfc3339());
    let dek = crypto::derive_scope_key(root_kek, scope);
    let envelope = crypto::seal(&dek, sentinel_value.as_bytes())?;
    secure_fs::write_bytes_atomic(&sentinel_path(data_root), &envelope)?;
    Ok(sentinel_value)
}

pub(crate) fn decrypt_sentinel(data_root: &Path, root_kek: &[u8; VAULT_KEK_LEN]) -> Result<String> {
    unlock::validate_credential_path(&sentinel_path(data_root))?;
    let bytes = std::fs::read(sentinel_path(data_root))?;
    decrypt_sentinel_bytes(&bytes, root_kek)
}

pub(crate) fn decrypt_sentinel_bytes(
    bytes: &[u8],
    root_kek: &[u8; VAULT_KEK_LEN],
) -> Result<String> {
    decrypt_sentinel_bytes_with_scope(
        bytes,
        root_kek,
        &unlock::credential_scope_label(SENTINEL_SCOPE)?,
    )
}

pub(crate) fn decrypt_sentinel_bytes_with_scope(
    bytes: &[u8],
    root_kek: &[u8; VAULT_KEK_LEN],
    scope: &str,
) -> Result<String> {
    let dek = crypto::derive_scope_key(root_kek, scope);
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
        let data_root = match unlock::process_vault_data_dir() {
            Ok(root) => root,
            Err(error) => {
                tracing::error!("Vault profile validation failed: {}", error);
                return;
            }
        };
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
