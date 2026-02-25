//! Hive API contracts — pure DTO types shared between hive-daemon, entity-daemon, and CLI clients.
//!
//! No business logic, no dependencies on `abigail-*` crates.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Generic API envelope
// ---------------------------------------------------------------------------

/// Standard JSON envelope for all Hive HTTP responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEnvelope<T> {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> ApiEnvelope<T> {
    pub fn success(data: T) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// Entity (agent) types
// ---------------------------------------------------------------------------

/// Serialized identity info for an entity (agent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityInfo {
    pub id: String,
    pub name: String,
    pub birth_complete: bool,
    pub birth_date: Option<String>,
}

/// Request to create a new entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEntityRequest {
    pub name: String,
}

/// Response after creating a new entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEntityResponse {
    pub id: String,
    pub directory: String,
}

// ---------------------------------------------------------------------------
// Provider config (Hive → Entity hand-off)
// ---------------------------------------------------------------------------

/// Resolved provider configuration that Hive hands to an Entity.
///
/// This is the serialized form of `HiveConfig` from `abigail-hive`.
/// The Entity uses it to construct its own LLM providers in-process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub local_llm_base_url: Option<String>,
    pub ego_provider_name: Option<String>,
    pub ego_api_key: Option<String>,
    pub ego_model: Option<String>,
    pub routing_mode: String,
    /// JSON-serialized TierModels (provider→model mappings for Fast/Standard/Pro).
    #[serde(default)]
    pub tier_models_json: Option<String>,
    /// Fast tier ceiling threshold (complexity scores below this → Fast).
    #[serde(default)]
    pub tier_threshold_fast_ceiling: Option<u8>,
    /// Pro tier floor threshold (complexity scores at/above this → Pro).
    #[serde(default)]
    pub tier_threshold_pro_floor: Option<u8>,
    /// JSON-serialized ForceOverride (pinned model/tier/provider).
    #[serde(default)]
    pub force_override_json: Option<String>,
}

// ---------------------------------------------------------------------------
// Hive status
// ---------------------------------------------------------------------------

/// Overall Hive status snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiveStatus {
    pub master_key_loaded: bool,
    pub entity_count: usize,
    pub entities: Vec<EntityInfo>,
}

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------

/// Request to store a secret in the Hive vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreSecretRequest {
    pub key: String,
    pub value: String,
}

/// Response listing secret names (values are never exposed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretListResponse {
    pub keys: Vec<String>,
}

/// Response returning a single secret value (localhost-only, for entity daemon startup).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretValueResponse {
    pub key: String,
    pub value: String,
}

// ---------------------------------------------------------------------------
// Sign request
// ---------------------------------------------------------------------------

/// Request to sign an entity's key after birth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignEntityRequest {
    pub entity_id: String,
}

// ---------------------------------------------------------------------------
// Model discovery
// ---------------------------------------------------------------------------

/// Request to discover models available from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModelsRequest {
    pub provider: String,
    pub api_key: String,
}

/// A single model available from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModelInfo {
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// Response listing models available from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModelsResponse {
    pub provider: String,
    pub models: Vec<ProviderModelInfo>,
}
