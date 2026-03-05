//! Vault subsystem: cross-platform encrypted secrets storage.
//!
//! This module provides:
//! - `crypto`   — AES-256-GCM envelope encrypt/decrypt + HKDF key derivation
//! - `unlock`   — hybrid unlock providers (OS credential store / passphrase / DPAPI)
//! - `scoped`   — Hive/entity/skills scoped vault built on the crypto layer
//! - `external` — read-only external pubkey vault (document signing, legacy)

pub mod crypto;
pub mod external;
pub mod scoped;
pub mod unlock;

pub use external::{ExternalVault, ReadOnlyFileVault};
pub use scoped::{ScopedVault, VaultScope};
pub use unlock::UnlockProvider;

/// Initialize vault/bootstrap logic on a blocking worker and recover from
/// decryption failures by resetting identity data only.
pub async fn init_resilient() {
    let result = tokio::task::spawn_blocking(|| {
        let data_root = crate::AppConfig::default_paths().data_dir;
        let _ = std::fs::create_dir_all(&data_root);

        if let Err(e) = crate::SecretsVault::load(data_root.clone()) {
            let err = e.to_string();
            if err.contains("AES-GCM") || err.contains("decryption failed") {
                tracing::warn!(
                    "Vault decryption failed (wrong key or tampered data). Auto-resetting identity folder for resilience."
                );

                // Never touch the Hive/documents folder during reset.
                let hive_docs = std::env::var("HIVE_DOCUMENTS_PATH")
                    .unwrap_or_else(|_| "hive/documents".to_string());
                if hive_docs.contains("superego_decisions.log") {
                    tracing::info!("Preserving Superego decision log during identity reset");
                }

                // Safe reset - only identity data.
                let identity_path = data_root.join("identities");
                if identity_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    == Some("identities")
                    && identity_path.exists()
                {
                    let _ = std::fs::remove_dir_all(&identity_path);
                }
            } else {
                tracing::error!("Vault bootstrap failed: {}", err);
            }
        }
    })
    .await;

    if let Err(e) = result {
        tracing::error!("Resilient vault bootstrap task failed to join: {}", e);
    }
}
