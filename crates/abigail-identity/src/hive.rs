use abigail_core::{AgentEntry, AppConfig, AutonomyProfile, CliPermissionMode, RoutingMode};
use std::path::{Path, PathBuf};

pub const HIVE_ENTITY_NAME: &str = "Abigail Hive";

#[derive(Debug, Clone)]
pub struct HiveEntity {
    pub id: String,
}

impl HiveEntity {
    pub fn new(id: String) -> Self {
        Self { id }
    }

    pub fn memory_db_path(data_root: &Path) -> PathBuf {
        data_root.join("memory.db")
    }

    pub fn locked_entitlements() -> &'static [&'static str] {
        &[
            "memory_owner",
            "registry_guardian",
            "local_privileged_runtime",
        ]
    }

    pub fn registry_entry(&self) -> AgentEntry {
        AgentEntry {
            id: self.id.clone(),
            name: HIVE_ENTITY_NAME.to_string(),
            is_hive: true,
            directory: PathBuf::from("identities").join(&self.id),
        }
    }

    pub fn config(&self, data_root: &Path, default_theme: &str) -> AppConfig {
        let agent_dir = data_root.join("identities").join(&self.id);
        let docs_dir = agent_dir.join("docs");

        AppConfig {
            schema_version: abigail_core::CONFIG_SCHEMA_VERSION,
            data_dir: agent_dir.clone(),
            models_dir: agent_dir.join("models"),
            docs_dir,
            db_path: Self::memory_db_path(data_root),
            openai_api_key: None,
            email: None,
            birth_complete: false,
            birth_stage: None,
            external_pubkey_path: None,
            local_llm_base_url: None,
            routing_mode: RoutingMode::default(),
            trinity: None,
            agent_name: Some(HIVE_ENTITY_NAME.to_string()),
            birth_timestamp: None,
            is_hive: true,
            mcp_servers: Vec::new(),
            mcp_trust_policy: Default::default(),
            approved_skill_ids: Vec::new(),
            trusted_skill_signers: Vec::new(),
            sao_endpoint: None,
            provider_catalog: Vec::new(),
            active_provider_preference: None,
            email_accounts: Vec::new(),
            bundled_ollama: true,
            bundled_model: Some("llama3.2:3b".to_string()),
            first_model_pull_complete: false,
            preloaded_skills_version: 0,
            primary_color: Some("#00c2a8".to_string()),
            avatar_url: None,
            share_skills_across_identities: false,
            allow_minor_visual_adaptation: false,
            allow_avatar_swap: false,
            memory_disclosure_enabled: true,
            forge_advanced_mode: false,
            signed_skill_allowlist: Vec::new(),
            known_recipients_by_identity: std::collections::HashMap::new(),
            skill_recovery_budget: 3,
            last_provider_change_at: None,
            cli_permission_mode: CliPermissionMode::DangerousSkipAll,
            runtime_mode: Default::default(),
            hive_daemon_url: "http://127.0.0.1:3141".to_string(),
            entity_daemon_url: "http://127.0.0.1:3142".to_string(),
            iggy_connection: None,
            theme_id: Some(default_theme.to_string()),
            autonomy_profile: AutonomyProfile::DesktopOperator,
        }
    }
}
