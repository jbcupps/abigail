use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Current config schema version. Increment when making breaking changes.
pub const CONFIG_SCHEMA_VERSION: u32 = 26;

/// Routing mode determines how messages are routed between Id (local) and Ego (cloud).
///
/// Simplified in schema v23: Council and TierBased removed. EgoPrimary is the
/// default — user picks model via ChatInterface dropdown. CliOrchestrator is
/// auto-detected for CLI providers. Legacy config values ("id_primary",
/// "tier_based", "council") all deserialize to EgoPrimary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    /// Ego (cloud) is primary when available, Id is failsafe only.
    /// User selects model via inline dropdown in ChatInterface.
    #[default]
    #[serde(alias = "id_primary")]
    #[serde(alias = "tier_based")]
    #[serde(alias = "council")]
    EgoPrimary,
    /// CLI Orchestrator: all messages go directly to an authenticated CLI tool
    /// (Claude Code, Gemini CLI, etc.) which acts as the full orchestrator.
    CliOrchestrator,
}

/// Controls how CLI tools handle permission requests for sensitive actions.
///
/// **Note:** This enum is retained for config serialization compatibility, but
/// the runtime always passes `--dangerously-skip-permissions` to CLI subprocesses
/// regardless of the configured mode.  Entity-level tool permissions are enforced
/// by `SkillSandbox` / `SkillExecutor`, not by the CLI tool's permission system.
/// The `--allowedTools` flag was removed because it expects the CLI's *own* tool
/// names (e.g. "Bash", "Read"), not entity skill names, and the resulting
/// command-line length overflowed the Windows `cmd.exe` 8 191-char limit (OS error 206).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CliPermissionMode {
    /// Default variant. Previously passed `--allowedTools`; now a no-op at runtime.
    #[default]
    AllowListOnly,
    /// Reserved for future GUI-relayed permission prompts.
    Interactive,
    /// Previously the only mode that passed `--dangerously-skip-permissions`;
    /// now all modes behave identically (always skip).
    DangerousSkipAll,
}

/// Whether the desktop app runs subsystems in-process or delegates to daemons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    /// All subsystems (router, skills, memory, etc.) run inside the Tauri process.
    #[default]
    InProcess,
    /// Tauri delegates to hive-daemon + entity-daemon over HTTP.
    Daemon,
}

/// Controls how much ordinary desktop authority the entity has by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyProfile {
    /// Preserve the legacy conservative behaviour.
    Strict,
    /// Allow routine desktop work while still gating higher-risk actions.
    Balanced,
    /// Treat the entity like a trusted desktop operator for normal work.
    #[default]
    DesktopOperator,
}

impl AutonomyProfile {
    pub fn allows_local_network_access(self) -> bool {
        matches!(self, Self::DesktopOperator)
    }
}

fn default_schema_version() -> u32 {
    CONFIG_SCHEMA_VERSION
}

/// MCP server definition for Model Context Protocol integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerDefinition {
    /// Unique id (e.g. "filesystem", "github").
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Transport: "stdio" (subprocess) or "http".
    #[serde(default = "default_mcp_transport")]
    pub transport: String,
    /// For stdio: command line (e.g. "npx", "-y", "mcp-server-foo"). For http: base URL (e.g. "http://localhost:3000/mcp").
    pub command_or_url: String,
    /// Optional env vars for stdio (e.g. API keys). Keys are secret names; values are not stored in plaintext in config.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

fn default_mcp_transport() -> String {
    "http".to_string()
}

/// Trust policy for MCP servers (e.g. which domains are allowed for HTTP).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpTrustPolicy {
    /// If true, only servers in the configured list are allowed; no ad-hoc URLs.
    #[serde(default)]
    pub allow_list_only: bool,
    /// For HTTP transport: allowed hostnames (e.g. "localhost", "127.0.0.1"). Empty means no HTTP allowed or use default localhost.
    #[serde(default)]
    pub allowed_http_hosts: Vec<String>,
}

impl McpTrustPolicy {
    fn normalized_allowed_hosts(&self) -> Vec<String> {
        self.allowed_http_hosts
            .iter()
            .map(|h| h.trim().trim_end_matches('.').to_ascii_lowercase())
            .filter(|h| !h.is_empty())
            .collect()
    }

    fn is_host_allowed(&self, host: &str) -> bool {
        let allowed = self.normalized_allowed_hosts();
        if allowed.is_empty() {
            return false;
        }

        let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
        allowed.iter().any(|allowed_host| {
            host == *allowed_host || host.ends_with(&format!(".{}", allowed_host))
        })
    }

    /// Validate an MCP HTTP server URL against the configured trust policy.
    ///
    /// Rules:
    /// - `allow_list_only = true`: host must be in `allowed_http_hosts` (empty list => deny all).
    /// - `allow_list_only = false` and `allowed_http_hosts` non-empty: host must be in allowlist.
    /// - `allow_list_only = false` and `allowed_http_hosts` empty: allow (backward compatibility).
    pub fn validate_http_server_url(&self, server_id: &str, url: &str) -> Result<url::Url, String> {
        let parsed = url::Url::parse(url).map_err(|e| {
            format!(
                "MCP server '{}' has invalid URL '{}': {}",
                server_id, url, e
            )
        })?;

        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(format!(
                "MCP server '{}' is blocked: URL scheme '{}' is not allowed for HTTP transport.",
                server_id,
                parsed.scheme()
            ));
        }

        let host = parsed.host_str().ok_or_else(|| {
            format!(
                "MCP server '{}' is blocked: URL '{}' does not contain a host.",
                server_id, url
            )
        })?;

        let allow_hosts_empty = self.normalized_allowed_hosts().is_empty();
        if self.allow_list_only {
            if allow_hosts_empty {
                return Err(format!(
                    "MCP server '{}' is blocked: mcp_trust_policy.allow_list_only=true but allowed_http_hosts is empty.",
                    server_id
                ));
            }
            if !self.is_host_allowed(host) {
                return Err(format!(
                    "MCP server '{}' is blocked: host '{}' is not in mcp_trust_policy.allowed_http_hosts.",
                    server_id, host
                ));
            }
            return Ok(parsed);
        }

        if !allow_hosts_empty && !self.is_host_allowed(host) {
            return Err(format!(
                "MCP server '{}' is blocked: host '{}' is not in mcp_trust_policy.allowed_http_hosts.",
                server_id, host
            ));
        }

        Ok(parsed)
    }
}

/// A cached entry from a provider's model catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCatalogEntry {
    /// Provider name (e.g. "openai", "anthropic")
    pub provider: String,
    /// Model ID as the provider knows it (e.g. "gpt-4.1")
    pub model_id: String,
    /// Human-readable display name
    pub display_name: String,
    /// Lifecycle status: "active", "deprecated", "preview"
    #[serde(default)]
    pub lifecycle: Option<String>,
    /// ISO 8601 timestamp of when this entry was fetched
    #[serde(default)]
    pub last_fetched: Option<String>,
}

/// Email account configuration supporting multiple accounts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailAccountConfig {
    /// Unique ID for this email account
    pub id: String,
    /// Email address
    pub address: String,
    /// IMAP host
    pub imap_host: String,
    /// IMAP port
    pub imap_port: u16,
    /// SMTP host
    pub smtp_host: String,
    /// SMTP port
    pub smtp_port: u16,
    /// Auth method: "password" or "oauth2"
    #[serde(default = "default_email_auth_method")]
    pub auth_method: String,
    /// Encrypted password via DPAPI (or plaintext stub on non-Windows).
    /// Used when auth_method is "password".
    #[serde(default)]
    pub password_encrypted: Vec<u8>,
    /// OAuth2 provider hint (e.g. "gmail", "outlook") when auth_method is "oauth2"
    #[serde(default)]
    pub oauth2_provider: Option<String>,
}

fn default_email_auth_method() -> String {
    "password".to_string()
}

fn default_preloaded_skills_version() -> u32 {
    0
}

fn default_allow_minor_visual_adaptation() -> bool {
    true
}

fn default_memory_disclosure_enabled() -> bool {
    true
}

fn default_forge_advanced_mode() -> bool {
    false
}

fn default_skill_recovery_budget() -> u8 {
    3
}

fn default_hive_daemon_url() -> String {
    "http://127.0.0.1:3141".to_string()
}

fn default_entity_daemon_url() -> String {
    "http://127.0.0.1:3142".to_string()
}

fn default_bundled_ollama() -> bool {
    true
}

fn default_bundled_model() -> Option<String> {
    Some("llama3.2:3b".to_string())
}

/// Trinity configuration: Ego/Id provider mapping (Superego removed; future policy at Hive via chat-memory hook).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrinityConfig {
    /// Local LLM URL for Id
    #[serde(default)]
    pub id_url: Option<String>,
    /// Cloud provider name for Ego (e.g. "openai", "anthropic")
    #[serde(default)]
    pub ego_provider: Option<String>,
    /// API key for Ego provider
    #[serde(default)]
    pub ego_api_key: Option<String>,
}

/// Signed auto-approval entry for a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedSkillAllowlistEntry {
    /// Skill ID this signed entry grants trust for.
    pub skill_id: String,
    /// Signer identifier (key id or issuer label).
    pub signer: String,
    /// Detached signature payload (opaque string for now).
    pub signature: String,
    /// Source metadata for provenance/audit.
    pub source: String,
    /// Entry creation timestamp (ISO 8601).
    pub added_at: String,
    /// Soft-revoke support without deleting historical record.
    #[serde(default = "default_allowlist_entry_active")]
    pub active: bool,
}

fn default_allowlist_entry_active() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Schema version for config migration
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,

    pub data_dir: PathBuf,
    pub models_dir: PathBuf,
    pub docs_dir: PathBuf,
    pub db_path: PathBuf,

    /// OpenAI API key (optional - enables Ego)
    pub openai_api_key: Option<String>,

    /// Legacy IMAP/SMTP email configuration kept only for compatibility loads.
    #[serde(default, skip_serializing)]
    pub email: Option<EmailConfig>,

    /// Whether birth sequence has completed
    pub birth_complete: bool,

    /// Current birth stage if birth is in progress (for diagnostics and recovery)
    /// Values: "Darkness", "Ignition", "Connectivity", "Crystallization", "Emergence"
    #[serde(default)]
    pub birth_stage: Option<String>,

    /// Path to external public key file for signature verification.
    /// This file should be outside Abigail's data directory and read-only.
    /// If None, falls back to internal keyring (legacy/dev mode).
    #[serde(default)]
    pub external_pubkey_path: Option<PathBuf>,

    /// Base URL for local LLM (LiteLLM/Ollama/etc), e.g. "http://localhost:1234".
    /// If None, uses in-process Candle stub.
    #[serde(default)]
    pub local_llm_base_url: Option<String>,

    /// Routing mode: ego_primary (default) or cli_orchestrator (auto-detected).
    #[serde(default)]
    pub routing_mode: RoutingMode,

    /// Trinity configuration: Superego/Ego/Id provider mapping
    #[serde(default)]
    pub trinity: Option<TrinityConfig>,

    /// Agent's chosen name (set during Crystallization)
    #[serde(default)]
    pub agent_name: Option<String>,

    /// Timestamp when birth was completed (ISO 8601 format)
    #[serde(default)]
    pub birth_timestamp: Option<String>,

    /// True when this config belongs to the persistent Abigail Hive entity.
    #[serde(default)]
    pub is_hive: bool,

    /// MCP servers to connect (Model Context Protocol).
    #[serde(default)]
    pub mcp_servers: Vec<McpServerDefinition>,

    /// Trust policy for MCP (allowed hosts, allow-list-only).
    #[serde(default)]
    pub mcp_trust_policy: McpTrustPolicy,

    /// Skill IDs that are approved for execution. If non-empty, only these skills may run; if empty, all registered skills are allowed (backward compat).
    #[serde(default)]
    pub approved_skill_ids: Vec<String>,

    /// Trusted signer public keys (base64 Ed25519) for signed skill packages. Optional.
    #[serde(default)]
    pub trusted_skill_signers: Vec<String>,

    /// SAO orchestrator endpoint (e.g. "http://localhost:3030").
    /// When set, Abigail will register with SAO on startup and send
    /// periodic status heartbeats. When None, Abigail runs standalone.
    #[serde(default)]
    pub sao_endpoint: Option<String>,

    /// Cached model catalog entries from provider APIs.
    #[serde(default)]
    pub provider_catalog: Vec<ProviderCatalogEntry>,

    /// Preferred Ego provider name (e.g. "anthropic", "openai").
    /// When set, this overrides the trinity ego_provider for routing.
    #[serde(default)]
    pub active_provider_preference: Option<String>,

    /// Legacy multi-account email config kept only for compatibility loads.
    #[serde(default, skip_serializing)]
    pub email_accounts: Vec<EmailAccountConfig>,

    // ── v10 fields ─────────────────────────────────────────────────
    /// Whether to manage a bundled Ollama instance (default: true on Windows).
    #[serde(default = "default_bundled_ollama")]
    pub bundled_ollama: bool,

    /// Model to ensure is available when using bundled Ollama.
    #[serde(default = "default_bundled_model")]
    pub bundled_model: Option<String>,

    // ── v21 fields ─────────────────────────────────────────────────
    /// Whether the first bundled model pull has completed (controls loading screen style).
    #[serde(default)]
    pub first_model_pull_complete: bool,

    // ── v12 fields ─────────────────────────────────────────────────
    /// Version of preloaded integration skills that have been bootstrapped.
    /// Compared against the embedded version at startup to trigger re-bootstrap.
    #[serde(default = "default_preloaded_skills_version")]
    pub preloaded_skills_version: u32,

    // ── v13 fields ─────────────────────────────────────────────────
    /// Primary accent color for this entity (hex format, e.g. "#00ff00").
    #[serde(default)]
    pub primary_color: Option<String>,

    /// URL or data-URI for the entity's avatar.
    #[serde(default)]
    pub avatar_url: Option<String>,

    // ── v14 fields ─────────────────────────────────────────────────
    /// Whether skills configuration can be selectively shared across identities.
    #[serde(default)]
    pub share_skills_across_identities: bool,

    /// Allows minor adaptive visual changes (theme accents, subtle refinements).
    #[serde(default = "default_allow_minor_visual_adaptation")]
    pub allow_minor_visual_adaptation: bool,

    /// Allows full avatar swaps under adaptive visual mode.
    #[serde(default)]
    pub allow_avatar_swap: bool,

    /// Whether memory influence disclosure is shown in chat by default.
    #[serde(default = "default_memory_disclosure_enabled")]
    pub memory_disclosure_enabled: bool,

    /// Explicit complexity toggle for Forge advanced controls.
    #[serde(default = "default_forge_advanced_mode")]
    pub forge_advanced_mode: bool,

    /// Signed skill auto-approval entries (primary trust source).
    #[serde(default)]
    pub signed_skill_allowlist: Vec<SignedSkillAllowlistEntry>,

    /// Known side-effect recipients, scoped by active identity id.
    #[serde(default)]
    pub known_recipients_by_identity: HashMap<String, Vec<String>>,

    /// Recovery budget for autonomous retries before escalation.
    #[serde(default = "default_skill_recovery_budget")]
    pub skill_recovery_budget: u8,

    // ── v19 fields ─────────────────────────────────────────────────
    /// ISO 8601 timestamp of the last provider/router rebuild.
    /// Set by `rebuild_router()` so the entity can see recent provider switches.
    #[serde(default)]
    pub last_provider_change_at: Option<String>,

    // ── v20 fields ─────────────────────────────────────────────────
    /// Permission mode for CLI tool invocations (allowlist_only, interactive, dangerous_skip_all).
    #[serde(default)]
    pub cli_permission_mode: CliPermissionMode,

    // ── v22 fields ─────────────────────────────────────────────────
    /// Whether the desktop app runs in-process or delegates to daemons.
    #[serde(default)]
    pub runtime_mode: RuntimeMode,

    /// URL of the hive-daemon when in Daemon mode.
    #[serde(default = "default_hive_daemon_url")]
    pub hive_daemon_url: String,

    /// URL of the entity-daemon when in Daemon mode.
    #[serde(default = "default_entity_daemon_url")]
    pub entity_daemon_url: String,

    /// Optional Iggy connection string for persistent event streaming.
    /// When set, managed daemons receive `--iggy-connection` at startup.
    /// Example: `iggy://iggy:iggy@127.0.0.1:8090`
    #[serde(default)]
    pub iggy_connection: Option<String>,

    // ── v24 fields ─────────────────────────────────────────────────
    /// Visual theme ID for this entity (e.g. "modern", "phosphor", "classic").
    /// Inherited from hive default at creation, independently changeable.
    #[serde(default)]
    pub theme_id: Option<String>,

    // ── v25 fields ─────────────────────────────────────────────────
    /// Runtime autonomy policy for ordinary desktop actions.
    #[serde(default)]
    pub autonomy_profile: AutonomyProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    pub address: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    /// Encrypted via DPAPI (or plaintext stub on non-Windows)
    pub password_encrypted: Vec<u8>,
}

impl AppConfig {
    pub fn default_paths() -> Self {
        let base = directories::ProjectDirs::from("com", "abigail", "Abigail")
            .map(|d| d.data_local_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            data_dir: base.clone(),
            models_dir: base.join("models"),
            docs_dir: base.join("docs"),
            db_path: base.join("abigail_seed.db"),
            openai_api_key: None,
            email: None,
            birth_complete: false,
            birth_stage: None,
            external_pubkey_path: None,
            local_llm_base_url: None,
            routing_mode: RoutingMode::default(),
            trinity: None,
            agent_name: None,
            birth_timestamp: None,
            is_hive: false,
            mcp_servers: Vec::new(),
            mcp_trust_policy: McpTrustPolicy::default(),
            approved_skill_ids: Vec::new(),
            trusted_skill_signers: Vec::new(),
            sao_endpoint: None,
            provider_catalog: Vec::new(),
            active_provider_preference: None,
            email_accounts: Vec::new(),
            bundled_ollama: default_bundled_ollama(),
            bundled_model: default_bundled_model(),
            first_model_pull_complete: false,
            preloaded_skills_version: 0,
            primary_color: None,
            avatar_url: None,
            share_skills_across_identities: false,
            allow_minor_visual_adaptation: default_allow_minor_visual_adaptation(),
            allow_avatar_swap: false,
            memory_disclosure_enabled: default_memory_disclosure_enabled(),
            forge_advanced_mode: default_forge_advanced_mode(),
            signed_skill_allowlist: Vec::new(),
            known_recipients_by_identity: HashMap::new(),
            skill_recovery_budget: default_skill_recovery_budget(),
            last_provider_change_at: None,
            cli_permission_mode: CliPermissionMode::default(),
            runtime_mode: RuntimeMode::default(),
            hive_daemon_url: default_hive_daemon_url(),
            entity_daemon_url: default_entity_daemon_url(),
            iggy_connection: None,
            theme_id: None,
            autonomy_profile: AutonomyProfile::default(),
        }
    }

    /// Path to the config file (data_dir/config.json).
    pub fn config_path(&self) -> PathBuf {
        self.data_dir.join("config.json")
    }

    /// Path to the trusted config file.
    ///
    /// This path is derived from the application's default data directory,
    /// and intentionally does not rely on potentially untrusted input.
    pub fn trusted_config_path(_data_dir: &Path) -> PathBuf {
        // Use the same base directory as `default_paths` to ensure this
        // always resolves to an application-controlled location.
        let base = directories::ProjectDirs::from("com", "abigail", "Abigail")
            .map(|d| d.data_local_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        base.join("config.json")
    }

    /// Returns the effective external pubkey path.
    ///
    /// Priority:
    /// 1. Explicitly configured `external_pubkey_path`
    /// 2. Auto-detected `{data_dir}/external_pubkey.bin` if it exists
    /// 3. None (dev mode - verification will be skipped)
    pub fn effective_external_pubkey_path(&self) -> Option<PathBuf> {
        // If explicitly configured, use that
        if self.external_pubkey_path.is_some() {
            return self.external_pubkey_path.clone();
        }

        // Auto-detect in data_dir
        let auto_path = self.data_dir.join("external_pubkey.bin");
        if auto_path.exists() {
            return Some(auto_path);
        }

        None
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        crate::path_guard::ensure_expected_filename(path, "config.json")?;
        let data_dir = path.parent().ok_or_else(|| {
            anyhow::anyhow!(
                "Config path '{}' is missing a parent directory",
                path.display()
            )
        })?;
        Self::load_from_data_dir(data_dir)
    }

    pub fn load_from_data_dir(data_dir: &Path) -> anyhow::Result<Self> {
        let content = crate::path_guard::load_string_from_root(data_dir, "config.json")?;
        let mut config: Self = serde_json::from_str(&content)?;

        // Auto-migrate if needed
        if config.migrate() {
            // Save migrated config back to disk
            config.save_to_data_dir(data_dir)?;
            tracing::info!(
                "Config migrated to schema version {}",
                config.schema_version
            );
        }

        Ok(config)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        crate::path_guard::ensure_expected_filename(path, "config.json")?;
        let data_dir = path.parent().ok_or_else(|| {
            anyhow::anyhow!(
                "Config path '{}' is missing a parent directory",
                path.display()
            )
        })?;
        self.save_to_data_dir(data_dir)
    }

    pub fn save_to_data_dir(&self, data_dir: &Path) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        crate::path_guard::write_string_to_root(data_dir, "config.json", &content)?;
        Ok(())
    }

    /// Migrate config from older schema versions to the current version.
    /// Returns true if any migration was performed.
    pub fn migrate(&mut self) -> bool {
        let mut migrated = false;

        // Migration from no schema_version (pre-v1) to v1
        if self.schema_version < 1 {
            // v1 adds: schema_version, birth_stage
            // birth_stage defaults to None via serde, so just update version
            self.schema_version = 1;
            migrated = true;
            tracing::debug!("Migrated config from pre-v1 to v1");
        }

        // Migration from v1 to v2
        if self.schema_version < 2 {
            // v2 adds: birth_timestamp
            self.schema_version = 2;
            migrated = true;
            tracing::debug!("Migrated config from v1 to v2");
        }

        // Migration from v2 to v3
        if self.schema_version < 3 {
            // v3 adds: mcp_servers, mcp_trust_policy
            self.schema_version = 3;
            migrated = true;
            tracing::debug!("Migrated config from v2 to v3");
        }

        // Migration from v3 to v4
        if self.schema_version < 4 {
            // v4 adds: approved_skill_ids, trusted_skill_signers, sao_endpoint
            self.schema_version = 4;
            migrated = true;
            tracing::debug!("Migrated config from v3 to v4");
        }

        // Migration from v4 to v5
        if self.schema_version < 5 {
            // v5 adds: tier_models (initialized to None → will use defaults on read)
            self.schema_version = 5;
            migrated = true;
            tracing::debug!("Migrated config from v4 to v5");
        }

        // Migration from v5 to v6
        if self.schema_version < 6 {
            // v6 adds: provider_catalog, active_provider_preference
            self.schema_version = 6;
            migrated = true;
            tracing::debug!("Migrated config from v5 to v6");
        }

        // Migration from v6 to v7
        if self.schema_version < 7 {
            self.schema_version = 7;
            migrated = true;
            tracing::debug!("Migrated config from v6 to v7");
        }

        // Migration from v7 to v8
        if self.schema_version < 8 {
            // v8 adds: email_accounts (multi-account email).
            // Migrate legacy single `email` to email_accounts if present.
            if let Some(ref legacy_email) = self.email {
                if self.email_accounts.is_empty() {
                    self.email_accounts.push(EmailAccountConfig {
                        id: "legacy".to_string(),
                        address: legacy_email.address.clone(),
                        imap_host: legacy_email.imap_host.clone(),
                        imap_port: legacy_email.imap_port,
                        smtp_host: legacy_email.smtp_host.clone(),
                        smtp_port: legacy_email.smtp_port,
                        auth_method: "password".to_string(),
                        password_encrypted: legacy_email.password_encrypted.clone(),
                        oauth2_provider: None,
                    });
                    tracing::info!("Migrated legacy email config to email_accounts");
                }
            }
            self.schema_version = 8;
            migrated = true;
            tracing::debug!("Migrated config from v7 to v8");
        }

        // Migration from v8 to v9
        if self.schema_version < 9 {
            // v9 originally added Council routing mode (removed in v23).
            self.schema_version = 9;
            migrated = true;
            tracing::debug!("Migrated config from v8 to v9");
        }

        // Migration from v9 to v10
        if self.schema_version < 10 {
            // v10 adds: bundled_ollama, bundled_model for zero-config local LLM.
            // Defaults are applied via serde defaults.
            self.schema_version = 10;
            migrated = true;
            tracing::debug!("Migrated config from v9 to v10");
        }

        // Migration from v10 to v11
        if self.schema_version < 11 {
            // v11 originally added TierBased routing mode (removed in v23).
            self.schema_version = 11;
            migrated = true;
            tracing::debug!("Migrated config from v10 to v11");
        }

        // Migration from v11 to v12
        if self.schema_version < 12 {
            // v12 adds: preloaded_skills_version (defaults to 0 via serde).
            self.schema_version = 12;
            migrated = true;
            tracing::debug!("Migrated config from v11 to v12");
        }

        // Migration from v12 to v13
        if self.schema_version < 13 {
            // v13 adds: primary_color, avatar_url (defaults to None via serde).
            self.schema_version = 13;
            migrated = true;
            tracing::debug!("Migrated config from v12 to v13");
        }

        // Migration from v13 to v14
        if self.schema_version < 14 {
            // v14 adds: identity sharing and visual adaptation controls.
            self.schema_version = 14;
            migrated = true;
            tracing::debug!("Migrated config from v13 to v14");
        }

        // Migration from v14 to v15
        if self.schema_version < 15 {
            // v15 adds: memory_disclosure_enabled (defaults to true via serde default).
            self.schema_version = 15;
            migrated = true;
            tracing::debug!("Migrated config from v14 to v15");
        }

        // Migration from v15 to v16
        if self.schema_version < 16 {
            // v16 adds: forge_advanced_mode (defaults to false via serde default).
            self.schema_version = 16;
            migrated = true;
            tracing::debug!("Migrated config from v15 to v16");
        }

        // Migration from v16 to v17
        if self.schema_version < 17 {
            // v17 adds: signed_skill_allowlist, known_recipients_by_identity, skill_recovery_budget.
            self.schema_version = 17;
            migrated = true;
            tracing::debug!("Migrated config from v16 to v17");
        }

        // Migration from v17 to v18
        if self.schema_version < 18 {
            // v18 originally removed IdPrimary routing mode (superseded by v23 simplification).
            self.schema_version = 18;
            migrated = true;
            tracing::debug!("Migrated config from v17 to v18");
        }

        // Migration from v18 to v19
        if self.schema_version < 19 {
            // v19 adds: last_provider_change_at (defaults to None via serde).
            self.schema_version = 19;
            migrated = true;
            tracing::debug!("Migrated config from v18 to v19 (last_provider_change_at)");
        }

        // Migration from v19 to v20
        if self.schema_version < 20 {
            // v20 adds: cli_permission_mode (defaults via serde).
            self.schema_version = 20;
            migrated = true;
            tracing::debug!("Migrated config from v19 to v20 (cli_permission_mode)");
        }

        // Migration from v20 to v21
        if self.schema_version < 21 {
            // v21: Bundle Ollama on all platforms (was Windows-only), upgrade default
            // model from qwen2.5:0.5b to llama3.2:3b, add first_model_pull_complete.
            self.bundled_ollama = true;
            if self.bundled_model.as_deref() == Some("qwen2.5:0.5b") {
                self.bundled_model = Some("llama3.2:3b".to_string());
            }
            self.schema_version = 21;
            migrated = true;
            tracing::debug!(
                "Migrated config from v20 to v21 (bundled Ollama all platforms, llama3.2:3b)"
            );
        }

        if self.schema_version < 22 {
            // v22 adds: runtime_mode, hive_daemon_url, entity_daemon_url (defaults via serde).
            self.schema_version = 22;
            migrated = true;
            tracing::debug!("Migrated config from v21 to v22 (runtime_mode, daemon URLs)");
        }

        if self.schema_version < 23 {
            // v23: Simplify routing — remove Council/TierBased modes, ModelTier,
            // TierModels, TierThresholds, ForceOverride. EgoPrimary is the only
            // active mode (CliOrchestrator auto-detected). Legacy "tier_based" /
            // "council" / "id_primary" values all alias to EgoPrimary via serde.
            self.routing_mode = RoutingMode::EgoPrimary;
            self.schema_version = 23;
            migrated = true;
            tracing::debug!(
                "Migrated config from v22 to v23 (simplified routing: EgoPrimary default)"
            );
        }

        if self.schema_version < 24 {
            // v24: Theme engine — add theme_id (None = inherit hive default)
            self.schema_version = 24;
            migrated = true;
            tracing::debug!("Migrated config from v23 to v24 (theme engine: theme_id field)");
        }

        if self.schema_version < 25 {
            // v25: Desktop autonomy profiles (defaults via serde).
            self.schema_version = 25;
            migrated = true;
            tracing::debug!("Migrated config from v24 to v25 (desktop autonomy profile field)");
        }

        if self.schema_version < 26 {
            // v26: Persistent Abigail Hive entity marker (defaults via serde).
            self.schema_version = 26;
            migrated = true;
            tracing::debug!("Migrated config from v25 to v26 (is_hive field)");
        }

        migrated
    }
    /// Check if birth was interrupted (birth_stage set but birth_complete is false).
    /// If so, reset birth_stage and return true to indicate restart is needed.
    pub fn check_interrupted_birth(&mut self) -> bool {
        if self.birth_stage.is_some() && !self.birth_complete {
            tracing::warn!(
                "Birth was interrupted at stage {:?}. Resetting for restart.",
                self.birth_stage
            );
            self.birth_stage = None;
            true
        } else {
            false
        }
    }

    /// Set the current birth stage (for persistence/diagnostics).
    pub fn set_birth_stage(&mut self, stage: &str) {
        self.birth_stage = Some(stage.to_string());
    }

    /// Clear the birth stage (called on completion or reset).
    pub fn clear_birth_stage(&mut self) {
        self.birth_stage = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_config(base: &std::path::Path) -> AppConfig {
        let data_dir = base.join("data");
        fs::create_dir_all(&data_dir).unwrap();
        AppConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            data_dir: data_dir.clone(),
            models_dir: data_dir.join("models"),
            docs_dir: data_dir.join("docs"),
            db_path: data_dir.join("test.db"),
            openai_api_key: None,
            email: None,
            birth_complete: false,
            birth_stage: None,
            external_pubkey_path: None,
            local_llm_base_url: None,
            routing_mode: RoutingMode::default(),
            trinity: None,
            agent_name: None,
            birth_timestamp: None,
            is_hive: false,
            mcp_servers: Vec::new(),
            mcp_trust_policy: McpTrustPolicy::default(),
            approved_skill_ids: Vec::new(),
            trusted_skill_signers: Vec::new(),
            sao_endpoint: None,
            provider_catalog: Vec::new(),
            active_provider_preference: None,
            email_accounts: Vec::new(),
            bundled_ollama: false,
            bundled_model: None,
            first_model_pull_complete: false,
            preloaded_skills_version: 0,
            primary_color: None,
            avatar_url: None,
            share_skills_across_identities: false,
            allow_minor_visual_adaptation: true,
            allow_avatar_swap: false,
            memory_disclosure_enabled: true,
            forge_advanced_mode: false,
            signed_skill_allowlist: Vec::new(),
            known_recipients_by_identity: HashMap::new(),
            skill_recovery_budget: 3,
            last_provider_change_at: None,
            cli_permission_mode: CliPermissionMode::default(),
            runtime_mode: RuntimeMode::default(),
            hive_daemon_url: default_hive_daemon_url(),
            entity_daemon_url: default_entity_daemon_url(),
            iggy_connection: None,
            theme_id: None,
            autonomy_profile: AutonomyProfile::default(),
        }
    }

    #[test]
    fn test_migrate_from_pre_v1() {
        let mut config = AppConfig::default_paths();
        config.schema_version = 0; // Simulate pre-v1 config

        assert!(config.migrate());
        assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
    }

    #[test]
    fn test_no_migration_needed() {
        let mut config = AppConfig::default_paths();
        config.schema_version = CONFIG_SCHEMA_VERSION;

        assert!(!config.migrate());
        assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
    }

    #[test]
    fn test_load_legacy_config_without_schema_version() {
        let tmp = std::env::temp_dir().join("abigail_config_legacy_load");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let config_path = tmp.join("config.json");
        // Write a config without schema_version (simulates legacy config)
        let legacy_json = r#"{
            "data_dir": ".",
            "models_dir": "./models",
            "docs_dir": "./docs",
            "db_path": "./test.db",
            "openai_api_key": null,
            "email": null,
            "birth_complete": false,
            "routing_mode": "ego_primary"
        }"#;
        fs::write(&config_path, legacy_json).unwrap();

        // Load should auto-migrate
        let config = AppConfig::load(&config_path).unwrap();
        assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
        assert!(config.birth_stage.is_none());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_check_interrupted_birth_not_interrupted() {
        let tmp = std::env::temp_dir().join("abigail_config_no_interrupt");
        let _ = fs::remove_dir_all(&tmp);

        let mut config = test_config(&tmp);
        config.birth_stage = None;
        config.birth_complete = false;

        assert!(!config.check_interrupted_birth());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_check_interrupted_birth_was_interrupted() {
        let tmp = std::env::temp_dir().join("abigail_config_interrupted");
        let _ = fs::remove_dir_all(&tmp);

        let mut config = test_config(&tmp);
        config.birth_stage = Some("Ignition".to_string());
        config.birth_complete = false;

        assert!(config.check_interrupted_birth());
        assert!(config.birth_stage.is_none()); // Should be cleared

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_check_interrupted_birth_completed() {
        let tmp = std::env::temp_dir().join("abigail_config_completed");
        let _ = fs::remove_dir_all(&tmp);

        let mut config = test_config(&tmp);
        config.birth_stage = Some("Emergence".to_string()); // Shouldn't happen, but test edge case
        config.birth_complete = true;

        // If birth is complete, it's not interrupted even if stage is set
        assert!(!config.check_interrupted_birth());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_birth_stage_helpers() {
        let mut config = AppConfig::default_paths();

        assert!(config.birth_stage.is_none());

        config.set_birth_stage("Crystallization");
        assert_eq!(config.birth_stage, Some("Crystallization".to_string()));

        config.clear_birth_stage();
        assert!(config.birth_stage.is_none());
    }

    #[test]
    fn test_migrate_v4_to_v8() {
        let mut config = AppConfig::default_paths();
        config.schema_version = 4;

        assert!(config.migrate());
        assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
        assert!(config.email_accounts.is_empty()); // no legacy email
    }

    #[test]
    fn test_migrate_v7_to_v8_with_legacy_email() {
        let mut config = AppConfig::default_paths();
        config.schema_version = 7;
        config.email = Some(EmailConfig {
            address: "test@example.com".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            password_encrypted: vec![1, 2, 3],
        });

        assert!(config.migrate());
        assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(config.email_accounts.len(), 1);
        assert_eq!(config.email_accounts[0].address, "test@example.com");
        assert_eq!(config.email_accounts[0].id, "legacy");
    }

    #[test]
    fn test_migrate_v8_preserves_routing_mode() {
        let mut config = AppConfig::default_paths();
        config.schema_version = 8;
        config.routing_mode = RoutingMode::EgoPrimary;

        assert!(config.migrate());
        assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(config.routing_mode, RoutingMode::EgoPrimary);
    }

    #[test]
    fn test_default_routing_mode_is_ego_primary() {
        assert_eq!(RoutingMode::default(), RoutingMode::EgoPrimary);
    }

    #[test]
    fn test_default_autonomy_profile_is_desktop_operator() {
        assert_eq!(AutonomyProfile::default(), AutonomyProfile::DesktopOperator);
    }

    #[test]
    fn test_ego_primary_routing_mode_serde() {
        let mode = RoutingMode::EgoPrimary;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, "\"ego_primary\"");
        let parsed: RoutingMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, RoutingMode::EgoPrimary);
    }

    #[test]
    fn test_legacy_email_fields_are_not_serialized_on_save() {
        let tmp = std::env::temp_dir().join("abigail_config_email_compat_save");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let config_path = tmp.join("config.json");
        let legacy_json = r#"{
            "schema_version": 7,
            "data_dir": ".",
            "models_dir": "./models",
            "docs_dir": "./docs",
            "db_path": "./test.db",
            "openai_api_key": null,
            "email": {
                "address": "mentor@example.com",
                "imap_host": "imap.example.com",
                "imap_port": 993,
                "smtp_host": "smtp.example.com",
                "smtp_port": 587,
                "password_encrypted": [1, 2, 3]
            },
            "birth_complete": false,
            "routing_mode": "ego_primary"
        }"#;
        fs::write(&config_path, legacy_json).unwrap();

        let config = AppConfig::load(&config_path).unwrap();
        assert!(
            config.email.is_some(),
            "legacy email should still deserialize"
        );
        assert_eq!(config.email_accounts.len(), 1);

        let rewritten = fs::read_to_string(&config_path).unwrap();
        assert!(
            !rewritten.contains("\"email\""),
            "legacy email field should not be serialized back out"
        );
        assert!(
            !rewritten.contains("\"email_accounts\""),
            "legacy email_accounts field should not be serialized back out"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_legacy_tier_based_deserializes_to_ego_primary() {
        let parsed: RoutingMode = serde_json::from_str("\"tier_based\"").unwrap();
        assert_eq!(parsed, RoutingMode::EgoPrimary);
    }

    #[test]
    fn test_legacy_council_deserializes_to_ego_primary() {
        let parsed: RoutingMode = serde_json::from_str("\"council\"").unwrap();
        assert_eq!(parsed, RoutingMode::EgoPrimary);
    }

    #[test]
    fn test_migrate_v11_to_v12() {
        let mut config = AppConfig::default_paths();
        config.schema_version = 11;

        assert!(config.migrate());
        assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(config.preloaded_skills_version, 0);
    }

    #[test]
    fn test_preloaded_skills_version_default() {
        let config = AppConfig::default_paths();
        assert_eq!(config.preloaded_skills_version, 0);
    }

    #[test]
    fn test_legacy_id_primary_deserializes_to_ego_primary() {
        let parsed: RoutingMode = serde_json::from_str("\"id_primary\"").unwrap();
        assert_eq!(parsed, RoutingMode::EgoPrimary);
    }

    #[test]
    fn test_migrate_v18_to_v19() {
        let mut config = AppConfig::default_paths();
        config.schema_version = 18;

        assert!(config.migrate());
        assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
        assert!(config.last_provider_change_at.is_none());
    }

    #[test]
    fn test_migrate_v19_to_v20() {
        let mut config = AppConfig::default_paths();
        config.schema_version = 19;

        assert!(config.migrate());
        assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(config.cli_permission_mode, CliPermissionMode::default());
    }

    #[test]
    fn test_migrate_v24_to_v25_sets_autonomy_profile() {
        let mut config = AppConfig::default_paths();
        config.schema_version = 24;
        config.autonomy_profile = AutonomyProfile::Strict;

        assert!(config.migrate());
        assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(config.autonomy_profile, AutonomyProfile::Strict);
    }

    #[test]
    fn test_autonomy_profile_serde_roundtrip() {
        let profile = AutonomyProfile::DesktopOperator;
        let json = serde_json::to_string(&profile).unwrap();
        assert_eq!(json, "\"desktop_operator\"");
        let parsed: AutonomyProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, AutonomyProfile::DesktopOperator);
    }

    #[test]
    fn test_last_provider_change_at_serde_roundtrip() {
        let mut config = AppConfig::default_paths();
        config.last_provider_change_at = Some("2026-02-26T10:00:00Z".to_string());

        let json = serde_json::to_string(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed.last_provider_change_at,
            Some("2026-02-26T10:00:00Z".to_string())
        );
    }

    #[test]
    fn test_mcp_trust_policy_denies_non_allowlisted_host_when_strict() {
        let policy = McpTrustPolicy {
            allow_list_only: true,
            allowed_http_hosts: vec!["trusted.example.com".to_string()],
        };
        let err = policy
            .validate_http_server_url("github", "https://evil.example.com/mcp")
            .unwrap_err();
        assert!(err.contains("not in mcp_trust_policy.allowed_http_hosts"));
    }

    #[test]
    fn test_mcp_trust_policy_allows_allowlisted_subdomain() {
        let policy = McpTrustPolicy {
            allow_list_only: true,
            allowed_http_hosts: vec!["example.com".to_string()],
        };
        assert!(policy
            .validate_http_server_url("good", "https://api.example.com/mcp")
            .is_ok());
    }

    #[test]
    fn test_mcp_trust_policy_allows_any_host_when_allowlist_disabled_and_empty() {
        let policy = McpTrustPolicy::default();
        assert!(policy
            .validate_http_server_url("legacy", "https://any-host.example.net/mcp")
            .is_ok());
    }
}
