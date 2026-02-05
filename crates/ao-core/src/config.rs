use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
            data_dir: base.clone(),
            models_dir: base.join("models"),
            docs_dir: base.join("docs"),
            db_path: base.join("ao_seed.db"),
            openai_api_key: None,
            email: None,
            birth_complete: false,
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
        Ok(serde_json::from_str(&content)?)
    }

    pub fn save(&self, path: &PathBuf) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }
}
