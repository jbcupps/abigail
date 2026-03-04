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
