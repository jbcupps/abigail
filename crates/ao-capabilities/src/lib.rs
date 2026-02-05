//! AO Capabilities — innate, high-trust body/brain functions.
//!
//! Capabilities are the biological analogy: cognitive (thinking), sensory (perceiving),
//! and memory (remembering). They have vault access and form the trusted core.

pub mod agent;
pub mod cognitive;
pub mod memory;
pub mod sensory;

use async_trait::async_trait;

/// The core Capability trait — high-trust components with vault access.
#[async_trait]
pub trait Capability: Send + Sync {
    /// Initialize the capability, optionally loading secrets from the vault.
    async fn initialize(
        &mut self,
        secrets: &mut ao_core::secrets::SecretsVault,
    ) -> anyhow::Result<()>;

    /// Shut down the capability gracefully.
    async fn shutdown(&mut self) -> anyhow::Result<()>;

    /// Human-readable name of this capability.
    fn name(&self) -> &str;
}
