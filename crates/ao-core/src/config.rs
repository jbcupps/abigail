use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Current config schema version. Increment when making breaking changes.
pub const CONFIG_SCHEMA_VERSION: u32 = 1;

/// Routing mode determines how messages are routed between Id (local) and Ego (cloud).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    /// Id (local) classifies, routes complex to Ego (legacy behavior)
    IdPrimary,
    /// Ego (cloud) is primary when available, Id is fallback (new default)
    #[default]
    EgoPrimary,
}

fn default_schema_version() -> u32 {
    CONFIG_SCHEMA_VERSION
}

/// Trinity configuration: maps providers to Superego/Ego/Id roles.
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
    /// Cloud provider name for Superego (e.g. "anthropic", "openai")
    #[serde(default)]
    pub superego_provider: Option<String>,
    /// API key for Superego provider
    #[serde(default)]
    pub superego_api_key: Option<String>,
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

    /// Email configuration for AO's account
    pub email: Option<EmailConfig>,

    /// Whether birth sequence has completed
    pub birth_complete: bool,

    /// Current birth stage if birth is in progress (for diagnostics and recovery)
    /// Values: "Darkness", "Ignition", "Connectivity", "Genesis", "Emergence"
    #[serde(default)]
    pub birth_stage: Option<String>,

    /// Path to external public key file for signature verification.
    /// This file should be outside AO's data directory and read-only.
    /// If None, falls back to internal keyring (legacy/dev mode).
    #[serde(default)]
    pub external_pubkey_path: Option<PathBuf>,

    /// Base URL for local LLM (LiteLLM/Ollama/etc), e.g. "http://localhost:1234".
    /// If None, uses in-process Candle stub.
    #[serde(default)]
    pub local_llm_base_url: Option<String>,

    /// Routing mode: ego_primary (default) or id_primary
    #[serde(default)]
    pub routing_mode: RoutingMode,

    /// Trinity configuration: Superego/Ego/Id provider mapping
    #[serde(default)]
    pub trinity: Option<TrinityConfig>,

    /// Agent's chosen name (set during Genesis)
    #[serde(default)]
    pub agent_name: Option<String>,
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
        let base = directories::ProjectDirs::from("com", "ao", "AO")
            .map(|d| d.data_local_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            data_dir: base.clone(),
            models_dir: base.join("models"),
            docs_dir: base.join("docs"),
            db_path: base.join("ao_seed.db"),
            openai_api_key: None,
            email: None,
            birth_complete: false,
            birth_stage: None,
            external_pubkey_path: None,
            local_llm_base_url: None,
            routing_mode: RoutingMode::default(),
            trinity: None,
            agent_name: None,
        }
    }

    /// Path to the config file (data_dir/config.json).
    pub fn config_path(&self) -> PathBuf {
        self.data_dir.join("config.json")
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

    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut config: Self = serde_json::from_str(&content)?;

        // Auto-migrate if needed
        if config.migrate() {
            // Save migrated config back to disk
            config.save(path)?;
            tracing::info!("Config migrated to schema version {}", config.schema_version);
        }

        Ok(config)
    }

    pub fn save(&self, path: &PathBuf) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
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

        // Future migrations:
        // if self.schema_version < 2 {
        //     // Migration logic for v2
        //     self.schema_version = 2;
        //     migrated = true;
        // }

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
        let tmp = std::env::temp_dir().join("ao_config_legacy_load");
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
        let tmp = std::env::temp_dir().join("ao_config_no_interrupt");
        let _ = fs::remove_dir_all(&tmp);

        let mut config = test_config(&tmp);
        config.birth_stage = None;
        config.birth_complete = false;

        assert!(!config.check_interrupted_birth());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_check_interrupted_birth_was_interrupted() {
        let tmp = std::env::temp_dir().join("ao_config_interrupted");
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
        let tmp = std::env::temp_dir().join("ao_config_completed");
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

        config.set_birth_stage("Genesis");
        assert_eq!(config.birth_stage, Some("Genesis".to_string()));

        config.clear_birth_stage();
        assert!(config.birth_stage.is_none());
    }
}
