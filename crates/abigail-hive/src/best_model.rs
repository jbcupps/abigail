//! Capability-ranked "best available model" selection.
//!
//! This is distinct from [`crate::hive::Hive::determine_ego_provider`], which
//! returns the *first* available provider by a fixed preference order. Here we
//! rank the providers the home actually has by capability tier and return the
//! strongest one — so the Hive helper and newly created entities can be pinned
//! to the best model available with zero per-entity setup.

use serde::{Deserialize, Serialize};

/// Relative capability of a provider's flagship model. Higher is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CapabilityTier {
    Local,
    Mid,
    High,
    Frontier,
}

impl CapabilityTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            CapabilityTier::Local => "local",
            CapabilityTier::Mid => "mid",
            CapabilityTier::High => "high",
            CapabilityTier::Frontier => "frontier",
        }
    }
}

/// The selected best provider/model and a short reason it was chosen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestModel {
    pub provider: String,
    /// Default flagship model id for API (keyed) providers; `None` for CLI
    /// providers, which manage their own model selection.
    pub model: Option<String>,
    pub tier: CapabilityTier,
    pub reason: String,
}

/// Capability tier and flagship default model for an API (keyed) provider.
/// The model defaults mirror `ProviderRegistry::build_ego_with_cli_mode`.
pub fn api_provider_profile(provider: &str) -> Option<(CapabilityTier, &'static str)> {
    match provider {
        "anthropic" => Some((CapabilityTier::Frontier, "claude-sonnet-4-6")),
        "openai" => Some((CapabilityTier::High, "gpt-4.1")),
        "google" => Some((CapabilityTier::High, "gemini-2.0-flash")),
        "xai" => Some((CapabilityTier::High, "grok-2")),
        "perplexity" => Some((CapabilityTier::Mid, "sonar")),
        _ => None,
    }
}

/// Capability tier for a CLI provider (authenticated via the tool itself).
pub fn cli_provider_tier(provider: &str) -> Option<CapabilityTier> {
    match provider {
        "claude-cli" => Some(CapabilityTier::Frontier),
        "codex-cli" => Some(CapabilityTier::High),
        "gemini-cli" => Some(CapabilityTier::High),
        "grok-cli" => Some(CapabilityTier::High),
        _ => None,
    }
}

/// All API providers the ranker knows about, strongest tier first.
pub const KNOWN_API_PROVIDERS: &[&str] = &["anthropic", "openai", "google", "xai", "perplexity"];
