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
    #[serde(default)]
    pub is_hive: bool,
    #[serde(default)]
    pub immortal: bool,
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
    /// CLI permission mode string (e.g. "allowlist_only", "interactive", "dangerous_skip_all").
    #[serde(default)]
    pub cli_permission_mode: Option<String>,
}

// ---------------------------------------------------------------------------
// Birth rite (Soul Forge)
// ---------------------------------------------------------------------------

/// One ethical trial in the Soul Forge, presented during the birth rite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeScenarioView {
    pub id: String,
    pub title: String,
    pub description: String,
    pub choices: Vec<ForgeChoiceView>,
}

/// One choice within a forge trial. Weight effects stay server-side so the
/// rite is a genuine values exercise, not a stat-min-maxing screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeChoiceView {
    pub id: String,
    pub label: String,
    pub description: String,
}

/// Response listing the Soul Forge trials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeScenariosResponse {
    pub scenarios: Vec<ForgeScenarioView>,
}

/// Request to perform (or re-run) the birth rite for an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BirthRiteRequest {
    /// "quickstart" (template soul, balanced weights) or "forge" (trials).
    pub path: String,
    /// (scenario_id, choice_id) pairs — required for the "forge" path.
    #[serde(default)]
    pub choices: Vec<(String, String)>,
}

/// The four ethical axes of an entity's soul.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EthicWeights {
    pub deontology: f32,
    pub teleology: f32,
    pub areteology: f32,
    pub welfare: f32,
}

/// A signed birth certificate — the entity's character sheet, vouched for by
/// the Hive master key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BirthCertificate {
    pub entity_id: String,
    pub entity_name: String,
    /// Soul archetype (e.g. "The Guardian").
    pub archetype: String,
    /// Flavor line for the archetype.
    pub epithet: String,
    pub weights: EthicWeights,
    /// Deterministic hash of the forged soul configuration.
    pub soul_hash: String,
    /// ASCII sigil art for the soul.
    pub sigil: String,
    /// "quickstart" or "forge".
    pub birth_path: String,
    pub issued_at_utc: String,
    /// Hex-encoded Hive master public key.
    pub master_public_key: String,
    /// Hex-encoded Ed25519 signature over the certificate payload.
    pub signature: String,
}

/// Response after performing the birth rite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BirthRiteResponse {
    pub certificate: BirthCertificate,
    /// True when the rite was re-run on an already-born entity (re-tempering).
    pub retempered: bool,
}

/// The full runtime identity for an entity, returned by
/// `GET /v1/entities/:id/birth`. Idempotent — entity-daemon re-reads it on
/// every launch so hive-side changes apply on restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityBirthDocument {
    pub entity: EntityInfo,
    #[serde(default)]
    pub certificate: Option<BirthCertificate>,
    pub provider_config: ProviderConfig,
    #[serde(default)]
    pub assignments: Vec<SkillAssignment>,
}

/// A named provider profile resolved by the Hive for sub-agent delegation.
///
/// Profiles let a sub-agent run on a different provider than the entity's
/// Ego (e.g. a research job on Perplexity while chat runs on Claude). The
/// profile name is a provider name; the Hive resolves credentials through
/// its vaults and environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderProfileResponse {
    /// The requested profile name.
    pub name: String,
    /// Resolved provider name (normalized).
    pub provider_name: String,
    /// API key, when the provider needs one (None for CLI/system auth).
    pub api_key: Option<String>,
    /// Default model for this profile (None = provider default).
    pub model: Option<String>,
}

/// Request to update per-entity provider and routing preferences.
///
/// Fields are patch-style: omitted values are left unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateEntityConfigRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_provider_preference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ego_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_llm_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_mode: Option<String>,
    /// CLI permission mode string
    /// (allowlist_only, interactive, dangerous_skip_all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_permission_mode: Option<String>,
}

/// Response after applying an entity config patch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateEntityConfigResponse {
    pub entity_id: String,
    pub provider_config: ProviderConfig,
}

// ---------------------------------------------------------------------------
// Hive status
// ---------------------------------------------------------------------------

/// Overall Hive status snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HiveStatus {
    pub master_key_loaded: bool,
    pub entity_count: usize,
    pub entities: Vec<EntityInfo>,
    /// Onboarding state: "starting" | "needs_provider" | "ready".
    #[serde(default)]
    pub ready_state: String,
    /// Whether at least one cloud or CLI provider is available.
    #[serde(default)]
    pub any_provider_configured: bool,
    /// Whether the home has ever completed provider setup.
    #[serde(default)]
    pub setup_complete: bool,
    /// Runtime status of the persistent Hive helper entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helper: Option<HelperInfo>,
}

/// Runtime status of the persistent Hive helper entity.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HelperInfo {
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_url: Option<String>,
}

/// Best available provider/model, ranked by capability. Fields are `None` when
/// no cloud or CLI provider is available (the home falls back to a local model).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BestModelResponse {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub tier: Option<String>,
    pub reason: Option<String>,
}

/// One detected CLI provider tool (claude, gemini, codex, grok).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliProviderDetection {
    pub provider: String,
    pub on_path: bool,
    pub is_official: bool,
    pub is_authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_hint: Option<String>,
}

/// Response for `GET /v1/providers/detect`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliDetectResponse {
    pub providers: Vec<CliProviderDetection>,
}

/// Set the Hive-level default provider/model that new entities inherit. Omit
/// both fields to seed from the best available provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SetHiveDefaultRequest {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

/// Response for `POST /v1/providers/hive-default`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiveDefaultResponse {
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// Response for `POST /v1/entities/:id/open` — the entity's daemon is running
/// and reachable at `local_url`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityOpenResponse {
    pub entity_id: String,
    pub local_url: String,
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

// ---------------------------------------------------------------------------
// Runtime session + supervision contracts
// ---------------------------------------------------------------------------

/// Request a Hive-issued runtime session lease for an entity runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSessionRequest {
    pub entity_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
}

/// Hive-issued session lease for a runtime instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSessionLease {
    pub lease_id: String,
    pub entity_id: String,
    pub runtime_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hive_url: Option<String>,
    pub issued_at_utc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at_utc: Option<String>,
    pub offline_until_close: bool,
    pub lease_scope: String,
}

/// Runtime registration payload sent after the runtime binds its local API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeRegistrationRequest {
    pub lease_id: String,
    pub runtime_id: String,
    pub local_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_id: Option<u32>,
}

/// Hive view of a registered runtime instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeRegistration {
    pub lease_id: String,
    pub runtime_id: String,
    pub entity_id: String,
    pub local_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_id: Option<u32>,
    pub registered_at_utc: String,
    pub last_seen_at_utc: String,
    pub state: String,
}

/// Combined session + runtime supervision status snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSessionStatus {
    pub lease: RuntimeSessionLease,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration: Option<RuntimeRegistration>,
    pub connected: bool,
    pub outbox_depth: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outbox_oldest_at_utc: Option<String>,
}

/// Heartbeat sent from the entity runtime to Hive while the runtime is alive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeHeartbeatRequest {
    pub lease_id: String,
    pub runtime_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_url: Option<String>,
    #[serde(default)]
    pub outbox_depth: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outbox_oldest_at_utc: Option<String>,
}

/// Heartbeat acknowledgement from Hive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeHeartbeatResponse {
    pub accepted: bool,
    pub server_time_utc: String,
}

// ---------------------------------------------------------------------------
// Skill assignments + forge approvals
// ---------------------------------------------------------------------------

/// A single Hive-managed skill assignment for an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillAssignment {
    pub assignment_id: String,
    pub entity_id: String,
    pub skill_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_path: Option<String>,
    pub assigned_at_utc: String,
    pub status: String,
}

/// Replace the set of skill assignments for an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetSkillAssignmentsRequest {
    pub assignments: Vec<SkillAssignment>,
}

/// Response wrapper for an entity's skill assignments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillAssignmentsResponse {
    pub entity_id: String,
    pub assignments: Vec<SkillAssignment>,
}

/// Hive-approved forge work item for a target entity runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeApprovalJob {
    pub job_id: String,
    pub entity_id: String,
    pub skill_id: String,
    pub code_path: String,
    pub markdown_path: String,
    pub approved_at_utc: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

/// Request to create a new forge approval for an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateForgeApprovalJobRequest {
    pub skill_id: String,
    pub code_path: String,
    pub markdown_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

/// Response wrapper for pending/known forge approvals for an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeApprovalJobsResponse {
    pub entity_id: String,
    pub jobs: Vec<ForgeApprovalJob>,
}

// ---------------------------------------------------------------------------
// Runtime outbox sync
// ---------------------------------------------------------------------------

/// Durable entity-scoped write queued locally while the runtime is active.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityOutboxRecord {
    pub record_id: String,
    pub entity_id: String,
    pub kind: String,
    pub created_at_utc: String,
    pub payload: serde_json::Value,
}

/// Batch sync request for runtime outbox records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxSyncRequest {
    pub lease_id: String,
    pub runtime_id: String,
    pub records: Vec<EntityOutboxRecord>,
}

/// Batch sync acknowledgement from Hive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxSyncResponse {
    pub accepted_record_ids: Vec<String>,
    pub pending_records: usize,
    pub server_time_utc: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_entity_config_request_serializes_patch_fields() {
        let patch = UpdateEntityConfigRequest {
            active_provider_preference: Some("openai".to_string()),
            ego_model: None,
            local_llm_base_url: Some("http://localhost:11434".to_string()),
            routing_mode: Some("ego_primary".to_string()),
            cli_permission_mode: Some("interactive".to_string()),
        };

        let json = serde_json::to_value(&patch).expect("serialize patch");
        assert_eq!(json["active_provider_preference"], "openai");
        assert_eq!(json["local_llm_base_url"], "http://localhost:11434");
        assert_eq!(json["routing_mode"], "ego_primary");
        assert_eq!(json["cli_permission_mode"], "interactive");
        assert!(json.get("ego_model").is_none());
    }
}
