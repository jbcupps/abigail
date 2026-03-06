//! Hive identity management — thread-safe manager for multi-agent identities.
//!
//! Extracted from `tauri-app/src/identity_manager.rs` as a standalone crate.
//! Zero Tauri imports — only uses `abigail_core`, `ed25519_dalek`, `serde`, `uuid`, `std`.

use abigail_core::{
    generate_master_key, load_master_key, sign_agent_key, verify_agent_signature, AgentEntry,
    AppConfig, GlobalConfig, SecretsVault,
};
use ed25519_dalek::{SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use uuid::Uuid;

/// Information about an agent identity for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentityInfo {
    pub id: String,
    pub name: String,
    pub directory: String,
    pub birth_complete: bool,
    pub birth_date: Option<String>,
}

/// Summary of an existing identity for the conflict screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentitySummary {
    pub name: String,
    pub birth_date: String,
    pub data_path: String,
    pub has_memories: bool,
    pub has_signatures: bool,
}

/// Metadata written into each backup directory as `backup_metadata.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadata {
    pub agent_id: String,
    pub agent_name: String,
    pub backup_type: String, // "manual_backup" | "archive"
    pub created_at: String,
    pub source_directory: String,
}

/// Information about a backup, returned to the frontend for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub directory_name: String,
    pub directory_path: String,
    pub agent_name: String,
    pub backup_type: String,
    pub created_at: String,
    pub birth_complete: bool,
    pub birth_date: Option<String>,
    pub has_memories: bool,
    pub has_signatures: bool,
}

/// The loaded context for a specific agent.
pub struct AgentContext {
    pub id: String,
    pub config: AppConfig,
    pub secrets: SecretsVault,
}

/// Thread-safe manager for multi-agent Hive identities.
///
/// Responsibilities:
/// - Load/store GlobalConfig (agent registry)
/// - Load/verify master key
/// - Create new agents (generate keys, sign with master)
/// - Load existing agents (verify signature, return context)
pub struct IdentityManager {
    data_root: PathBuf,
    global_config: RwLock<GlobalConfig>,
    master_key: SigningKey,
}

impl IdentityManager {
    fn normalize_agent_directory(&self, directory: &Path) -> Result<PathBuf, String> {
        abigail_core::global_config::normalize_agent_directory(directory).map_err(|e| e.to_string())
    }

    fn resolve_agent_dir(&self, directory: &Path) -> Result<PathBuf, String> {
        let relative = self.normalize_agent_directory(directory)?;
        abigail_core::path_guard::resolve_within_root(
            &self.data_root,
            &relative,
            "agent directory",
        )
        .map_err(|e| e.to_string())
    }

    fn registry_directory_for(&self, agent_id: &str) -> PathBuf {
        PathBuf::from("identities").join(agent_id)
    }

    /// Create a new IdentityManager, loading GlobalConfig and master key from disk.
    /// If master key doesn't exist, generates one (first-run bootstrap).
    pub fn new(data_root: PathBuf) -> anyhow::Result<Self> {
        // paper Sections 22-27 runtime verification:
        // the KEK must already be session-verified before identity bootstrap.
        let unlock = abigail_core::HybridUnlockProvider::new();
        use abigail_core::UnlockProvider as _;
        unlock
            .root_kek()
            .map_err(|e| anyhow::anyhow!(format!("Recovery Mode: {}", e)))?;

        Self::try_load_from_vault(data_root).map_err(|e| {
            tracing::error!("Fatal identity error: {}", e);
            anyhow::anyhow!(e)
        })
    }

    fn try_load_from_vault(data_root: PathBuf) -> Result<Self, String> {
        // Ensure directories exist
        std::fs::create_dir_all(&data_root).map_err(|e| e.to_string())?;
        let identities_dir = data_root.join("identities");
        std::fs::create_dir_all(&identities_dir).map_err(|e| e.to_string())?;

        // Load or create master key
        let master_key_path = data_root.join("master.key");
        let master_key = if master_key_path.exists() {
            load_master_key(&master_key_path).map_err(|e| e.to_string())?
        } else {
            tracing::info!("No master key found, generating new Hive master key");
            // Use the signing key returned directly from generate_master_key().
            // Previously we called load_master_key() immediately after, which
            // created a second HybridUnlockProvider — if the OS credential store
            // failed to persist the KEK, the second provider would generate a
            // *different* random KEK, causing AES-GCM decryption to fail.
            let result = generate_master_key(&data_root).map_err(|e| e.to_string())?;
            result.signing_key
        };

        // Load or create global config
        let global_config = if GlobalConfig::config_path(&data_root).exists() {
            GlobalConfig::load(&data_root).map_err(|e| e.to_string())?
        } else {
            let config = GlobalConfig::new(&data_root);
            config.save(&data_root).map_err(|e| e.to_string())?;
            config
        };

        Ok(Self {
            data_root,
            global_config: RwLock::new(global_config),
            master_key,
        })
    }

    /// Get the data root path.
    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    /// Access the global config for hive-level settings.
    pub fn global_config(&self) -> &RwLock<GlobalConfig> {
        &self.global_config
    }

    /// Get the identities directory path.
    pub fn identities_dir(&self) -> PathBuf {
        self.data_root.join("identities")
    }

    /// List all registered agents with their info.
    pub fn list_agents(&self) -> Result<Vec<AgentIdentityInfo>, String> {
        let gc = self.global_config.read().map_err(|e| e.to_string())?;
        let mut agents = Vec::new();

        for entry in &gc.agents {
            let agent_dir = self.resolve_agent_dir(&entry.directory)?;

            let config_path = agent_dir.join("config.json");
            let (birth_complete, birth_date) = if config_path.exists() {
                match AppConfig::load(&config_path) {
                    Ok(config) => (config.birth_complete, config.birth_timestamp.clone()),
                    Err(_) => (false, None),
                }
            } else {
                (false, None)
            };

            agents.push(AgentIdentityInfo {
                id: entry.id.clone(),
                name: entry.name.clone(),
                directory: agent_dir.to_string_lossy().to_string(),
                birth_complete,
                birth_date,
            });
        }

        Ok(agents)
    }

    /// Verify an agent's signature against the master key.
    /// Returns Ok(()) if valid, Err with message if invalid.
    pub fn verify_agent(&self, agent_id: &str) -> Result<(), String> {
        let gc = self.global_config.read().map_err(|e| e.to_string())?;
        let entry = gc
            .find_agent(agent_id)
            .ok_or_else(|| format!("Agent {} not registered", agent_id))?;

        let agent_dir = self.resolve_agent_dir(&entry.directory)?;

        // Read the agent's public key
        let pubkey_path = agent_dir.join("external_pubkey.bin");
        if !pubkey_path.exists() {
            return Err(format!(
                "Agent {} has no public key at {}",
                agent_id,
                pubkey_path.display()
            ));
        }
        let pubkey_bytes = std::fs::read(&pubkey_path).map_err(|e| e.to_string())?;
        let pubkey_array: [u8; 32] = pubkey_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "Invalid public key length")?;
        let agent_pubkey = VerifyingKey::from_bytes(&pubkey_array)
            .map_err(|e| format!("Invalid public key: {}", e))?;

        // Read the signature
        let sig_path = agent_dir.join("signature.sig");
        if !sig_path.exists() {
            return Err(format!(
                "Agent {} has no Hive signature at {}",
                agent_id,
                sig_path.display()
            ));
        }
        let sig_bytes = std::fs::read(&sig_path).map_err(|e| e.to_string())?;

        // Verify
        let master_pubkey = self.master_key.verifying_key();
        if !verify_agent_signature(&master_pubkey, &agent_pubkey, &sig_bytes) {
            return Err(format!(
                "SECURITY: Agent {} signature verification FAILED. This agent may not belong to this Hive.",
                agent_id
            ));
        }

        Ok(())
    }

    /// Load an agent by UUID. Verifies signature for born agents, skips for unborn.
    pub fn load_agent(&self, agent_id: &str) -> Result<AppConfig, String> {
        let gc = self.global_config.read().map_err(|e| e.to_string())?;
        let entry = gc
            .find_agent(agent_id)
            .ok_or_else(|| format!("Agent {} not registered", agent_id))?;

        let agent_dir = self.resolve_agent_dir(&entry.directory)?;

        let config_path = agent_dir.join("config.json");
        if !config_path.exists() {
            return Err(format!("Agent {} config not found", agent_id));
        }

        let config = AppConfig::load(&config_path).map_err(|e| e.to_string())?;

        // Only verify signature for born agents (unborn agents don't have keys yet)
        if config.birth_complete {
            drop(gc); // Release read lock before calling verify_agent
            self.verify_agent(agent_id)?;
        } else {
            tracing::info!(
                "Skipping signature verification for unborn agent {}",
                agent_id
            );
        }

        // Ensure per-agent Documents folder exists
        let _ = self.create_documents_folder(agent_id);

        Ok(config)
    }

    /// Create a new agent. Generates UUID and directory structure only.
    /// Keypair generation and signing are deferred to the birth sequence.
    /// Returns (uuid, agent_dir).
    pub fn create_agent(&self, name: &str) -> Result<(String, PathBuf), String> {
        let uuid = Uuid::new_v4().to_string();
        let agent_dir = self.identities_dir().join(&uuid);

        // Create agent directory structure
        std::fs::create_dir_all(&agent_dir).map_err(|e| e.to_string())?;
        let docs_dir = agent_dir.join("docs");
        std::fs::create_dir_all(&docs_dir).map_err(|e| e.to_string())?;

        // Create agent-specific config (no keypair yet — birth will generate it)
        let config = AppConfig {
            schema_version: abigail_core::CONFIG_SCHEMA_VERSION,
            data_dir: agent_dir.clone(),
            models_dir: agent_dir.join("models"),
            docs_dir: docs_dir.clone(),
            db_path: agent_dir.join("abigail_memory.db"),
            openai_api_key: None,
            email: None,
            birth_complete: false,
            birth_stage: None,
            external_pubkey_path: None,
            local_llm_base_url: None,
            routing_mode: abigail_core::RoutingMode::default(),
            trinity: None,
            agent_name: Some(name.to_string()),
            birth_timestamp: None,
            mcp_servers: Default::default(),
            mcp_trust_policy: Default::default(),
            approved_skill_ids: Default::default(),
            trusted_skill_signers: Default::default(),
            sao_endpoint: None,
            provider_catalog: Vec::new(),
            active_provider_preference: None,
            email_accounts: Vec::new(),
            bundled_ollama: true,
            bundled_model: Some("llama3.2:3b".to_string()),
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
            known_recipients_by_identity: std::collections::HashMap::new(),
            skill_recovery_budget: 3,
            last_provider_change_at: None,
            cli_permission_mode: Default::default(),
            runtime_mode: Default::default(),
            hive_daemon_url: "http://127.0.0.1:3141".to_string(),
            entity_daemon_url: "http://127.0.0.1:3142".to_string(),
            iggy_connection: None,
            theme_id: {
                let gc = self.global_config.read().map_err(|e| e.to_string())?;
                Some(gc.default_theme.clone())
            },
            autonomy_profile: Default::default(),
        };
        let config_path = agent_dir.join("config.json");
        config.save(&config_path).map_err(|e| e.to_string())?;

        // Register in global config
        {
            let mut gc = self.global_config.write().map_err(|e| e.to_string())?;
            gc.register_agent(AgentEntry {
                id: uuid.clone(),
                name: name.to_string(),
                directory: self.registry_directory_for(&uuid),
            })
            .map_err(|e| e.to_string())?;
            gc.save(&self.data_root).map_err(|e| e.to_string())?;
        }

        // Create per-agent Documents folder
        let _ = self.create_documents_folder(&uuid);

        tracing::info!("Created new agent: {} ({})", name, uuid);
        Ok((uuid, agent_dir))
    }

    /// Sign an agent's public key with the Hive master key after birth completes.
    /// Called after BootSequence finishes — the agent now has `external_pubkey.bin`
    /// from the birth-generated keypair.
    pub fn sign_agent_after_birth(&self, agent_id: &str) -> Result<(), String> {
        let gc = self.global_config.read().map_err(|e| e.to_string())?;
        let entry = gc
            .find_agent(agent_id)
            .ok_or_else(|| format!("Agent {} not registered", agent_id))?;

        let agent_dir = self.resolve_agent_dir(&entry.directory)?;
        drop(gc);

        // Read the agent's public key (generated during birth)
        let pubkey_path = agent_dir.join("external_pubkey.bin");
        if !pubkey_path.exists() {
            return Err(format!(
                "Agent {} has no public key — birth may not have completed keypair generation",
                agent_id
            ));
        }

        let pubkey_bytes = std::fs::read(&pubkey_path).map_err(|e| e.to_string())?;
        let pubkey_array: [u8; 32] = pubkey_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "Invalid public key length")?;
        let agent_pubkey = VerifyingKey::from_bytes(&pubkey_array)
            .map_err(|e| format!("Invalid public key: {}", e))?;

        // Sign with master key and write signature
        let signature = sign_agent_key(&self.master_key, &agent_pubkey);
        let sig_path = agent_dir.join("signature.sig");
        std::fs::write(&sig_path, &signature).map_err(|e| e.to_string())?;

        tracing::info!(
            "Signed agent {} with Hive master key (post-birth)",
            agent_id
        );
        Ok(())
    }

    /// Get the agent directory path for a given UUID.
    pub fn agent_dir(&self, agent_id: &str) -> Result<PathBuf, String> {
        let gc = self.global_config.read().map_err(|e| e.to_string())?;
        let entry = gc
            .find_agent(agent_id)
            .ok_or_else(|| format!("Agent {} not registered", agent_id))?;

        let agent_dir = self.resolve_agent_dir(&entry.directory)?;
        Ok(agent_dir)
    }

    /// Update an agent's name in the global config.
    pub fn update_agent_name(&self, agent_id: &str, new_name: &str) -> Result<(), String> {
        let mut gc = self.global_config.write().map_err(|e| e.to_string())?;
        if let Some(entry) = gc.agents.iter_mut().find(|a| a.id == agent_id) {
            entry.name = new_name.to_string();
            gc.save(&self.data_root).map_err(|e| e.to_string())?;
            Ok(())
        } else {
            Err(format!("Agent {} not registered", agent_id))
        }
    }

    /// Create (or ensure existence of) the per-agent Documents folder.
    /// On Windows: `%USERPROFILE%\Documents\Abigail\{agent_name}\`
    /// On other platforms: `~/Documents/Abigail/{agent_name}/`
    pub fn create_documents_folder(&self, agent_id: &str) -> Result<PathBuf, String> {
        let agent_name = {
            let gc = self.global_config.read().map_err(|e| e.to_string())?;
            let entry = gc
                .find_agent(agent_id)
                .ok_or_else(|| format!("Agent {} not registered", agent_id))?;
            entry.name.clone()
        };

        // Sanitize the name for use as a directory (replace problematic chars)
        let safe_name: String = agent_name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let safe_name = safe_name.trim().to_string();
        let safe_name = if safe_name.is_empty() {
            agent_id.to_string()
        } else {
            safe_name
        };

        #[cfg(windows)]
        let docs_base = {
            let profile = std::env::var("USERPROFILE")
                .map_err(|_| "USERPROFILE environment variable not set")?;
            PathBuf::from(profile).join("Documents")
        };
        #[cfg(not(windows))]
        let docs_base = {
            let home = std::env::var("HOME").map_err(|_| "HOME environment variable not set")?;
            PathBuf::from(home).join("Documents")
        };

        let agent_docs = docs_base.join("Abigail").join(&safe_name);
        std::fs::create_dir_all(&agent_docs)
            .map_err(|e| format!("Failed to create Documents folder: {}", e))?;

        tracing::info!("Documents folder ready: {}", agent_docs.display());
        Ok(agent_docs)
    }

    /// Save the recovery key to a text file in the agent's Documents folder.
    pub fn save_recovery_key(&self, agent_id: &str, private_key: &str) -> Result<String, String> {
        let docs_dir = self.create_documents_folder(agent_id)?;
        let file_path = docs_dir.join("RECOVERY_KEY.txt");

        let content = format!(
            "ABIGAIL RECOVERY KEY\n\
             ====================\n\n\
             This is your Ed25519 Private Signing Key. Keep it secure!\n\
             It is used to verify and re-sign your agent's constitutional documents.\n\n\
             AGENT ID: {}\n\
             PRIVATE KEY (Base64): {}\n\n\
             Date Saved: {}\n",
            agent_id,
            private_key,
            chrono::Utc::now().to_rfc3339()
        );

        std::fs::write(&file_path, content)
            .map_err(|e| format!("Failed to write recovery key file: {}", e))?;

        Ok(file_path.to_string_lossy().to_string())
    }

    /// Check if any agents exist.
    pub fn has_agents(&self) -> bool {
        self.global_config
            .read()
            .map(|gc| !gc.agents.is_empty())
            .unwrap_or(false)
    }

    /// Try to detect and migrate a legacy single-identity installation.
    /// Returns the migrated agent UUID if successful, None if no legacy identity found.
    pub fn migrate_legacy_identity(&self) -> Result<Option<String>, String> {
        // Check for legacy identity markers in data_root
        let legacy_config_path = self.data_root.join("config.json");
        let legacy_pubkey = self.data_root.join("external_pubkey.bin");

        if !legacy_config_path.exists() {
            return Ok(None); // No legacy identity
        }

        let legacy_config = AppConfig::load(&legacy_config_path).map_err(|e| e.to_string())?;

        // Only migrate if birth was complete
        if !legacy_config.birth_complete {
            return Ok(None);
        }

        let agent_name = legacy_config
            .agent_name
            .clone()
            .unwrap_or_else(|| "Legacy Agent".to_string());

        tracing::info!("Migrating legacy identity '{}' to Hive format", agent_name);

        let uuid = Uuid::new_v4().to_string();
        let agent_dir = self.identities_dir().join(&uuid);
        std::fs::create_dir_all(&agent_dir).map_err(|e| e.to_string())?;

        // Copy legacy files into the agent directory. Include both the older
        // `.bin` payloads and the newer `.vault`/DB names so installs upgraded
        // across multiple Abigail versions keep their working key material.
        let files_to_copy = [
            "config.json",
            "keys.bin",
            "keys.vault",
            "external_pubkey.bin",
            "secrets.bin",
            "secrets.vault",
            "skills.bin",
            "skills.vault",
            "vault.sentinel",
            "abigail_seed.db",
            "abigail_seed.db-wal",
            "abigail_seed.db-shm",
            "abigail_memory.db",
            "abigail_memory.db-wal",
            "abigail_memory.db-shm",
            "jobs.db",
            "jobs.db-wal",
            "jobs.db-shm",
        ];

        for file in &files_to_copy {
            let src = self.data_root.join(file);
            let dst = agent_dir.join(file);
            if src.exists() {
                std::fs::copy(&src, &dst)
                    .map_err(|e| format!("Failed to copy legacy file {}: {}", file, e))?;
            }
        }

        // Move docs/ directory
        let legacy_docs = self.data_root.join("docs");
        let agent_docs = agent_dir.join("docs");
        if legacy_docs.exists() && !agent_docs.exists() {
            copy_dir_recursive(&legacy_docs, &agent_docs)
                .map_err(|e| format!("Failed to copy legacy docs: {}", e))?;
        }

        // Update agent config paths
        let agent_config_path = agent_dir.join("config.json");
        if agent_config_path.exists() {
            let mut config = AppConfig::load(&agent_config_path).map_err(|e| e.to_string())?;
            config.data_dir = agent_dir.clone();
            config.models_dir = agent_dir.join("models");
            config.docs_dir = agent_dir.join("docs");
            config.db_path = if agent_dir.join("abigail_memory.db").exists() {
                agent_dir.join("abigail_memory.db")
            } else {
                agent_dir.join("abigail_seed.db")
            };
            if legacy_pubkey.exists() && agent_dir.join("external_pubkey.bin").exists() {
                config.external_pubkey_path = Some(agent_dir.join("external_pubkey.bin"));
            }
            config.save(&agent_config_path).map_err(|e| e.to_string())?;
        }

        // Sign the agent's public key with master key (if pubkey exists)
        let agent_pubkey_path = agent_dir.join("external_pubkey.bin");
        if agent_pubkey_path.exists() {
            let pubkey_bytes = std::fs::read(&agent_pubkey_path).map_err(|e| e.to_string())?;
            if pubkey_bytes.len() == 32 {
                let pubkey_array: [u8; 32] = pubkey_bytes.as_slice().try_into().unwrap();
                if let Ok(agent_pubkey) = VerifyingKey::from_bytes(&pubkey_array) {
                    let signature = sign_agent_key(&self.master_key, &agent_pubkey);
                    let sig_path = agent_dir.join("signature.sig");
                    std::fs::write(&sig_path, &signature).map_err(|e| e.to_string())?;
                }
            }
        }

        // Register in global config
        {
            let mut gc = self.global_config.write().map_err(|e| e.to_string())?;
            gc.register_agent(AgentEntry {
                id: uuid.clone(),
                name: agent_name.clone(),
                directory: self.registry_directory_for(&uuid),
            })
            .map_err(|e| e.to_string())?;
            gc.save(&self.data_root).map_err(|e| e.to_string())?;
        }

        tracing::info!(
            "Legacy identity '{}' migrated to agent {}",
            agent_name,
            uuid
        );
        Ok(Some(uuid))
    }

    /// Delete an agent and all its data from disk.
    pub fn delete_agent(&self, agent_id: &str) -> Result<(), String> {
        let agent_dir = {
            let mut gc = self.global_config.write().map_err(|e| e.to_string())?;
            let entry = gc
                .find_agent(agent_id)
                .ok_or_else(|| format!("Agent {} not registered", agent_id))?
                .clone();

            let dir = self.resolve_agent_dir(&entry.directory)?;

            gc.remove_agent(agent_id);
            gc.save(&self.data_root).map_err(|e| e.to_string())?;
            dir
        };

        if agent_dir.exists() {
            tracing::info!("Deleting agent data directory: {}", agent_dir.display());
            std::fs::remove_dir_all(&agent_dir)
                .map_err(|e| format!("Failed to delete agent directory: {}", e))?;
        }

        Ok(())
    }

    /// Archive an agent by moving its directory to Hive backups and
    /// removing it from the active registry.
    pub fn archive_agent(&self, agent_id: &str) -> Result<String, String> {
        let (agent_name, agent_dir) = {
            let mut gc = self.global_config.write().map_err(|e| e.to_string())?;
            let entry = gc
                .find_agent(agent_id)
                .ok_or_else(|| format!("Agent {} not registered", agent_id))?
                .clone();

            let dir = self.resolve_agent_dir(&entry.directory)?;

            gc.remove_agent(agent_id);
            gc.save(&self.data_root).map_err(|e| e.to_string())?;
            (entry.name, dir)
        };

        if !agent_dir.exists() {
            return Err(format!(
                "Agent directory not found: {}",
                agent_dir.display()
            ));
        }

        let backups_dir = self.data_root.join("backups");
        std::fs::create_dir_all(&backups_dir)
            .map_err(|e| format!("Failed to create backups directory: {}", e))?;

        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let safe_name =
            agent_name.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_");
        let backup_name = format!("{}_{}", timestamp, safe_name);
        let backup_path = backups_dir.join(backup_name);

        std::fs::rename(&agent_dir, &backup_path)
            .map_err(|e| format!("Failed to archive agent directory: {}", e))?;

        // Best-effort: write backup metadata into the archive
        let metadata = BackupMetadata {
            agent_id: agent_id.to_string(),
            agent_name: agent_name.clone(),
            backup_type: "archive".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            source_directory: agent_dir.to_string_lossy().to_string(),
        };
        let metadata_path = backup_path.join("backup_metadata.json");
        if let Ok(json) = serde_json::to_string_pretty(&metadata) {
            let _ = std::fs::write(&metadata_path, json);
        }

        Ok(backup_path.to_string_lossy().to_string())
    }

    /// Create a non-destructive backup of an agent's data directory.
    /// The agent stays active — this copies (not moves) the data.
    pub fn backup_agent(&self, agent_id: &str) -> Result<String, String> {
        let (agent_name, agent_dir) = {
            let gc = self.global_config.read().map_err(|e| e.to_string())?;
            let entry = gc
                .find_agent(agent_id)
                .ok_or_else(|| format!("Agent {} not registered", agent_id))?
                .clone();

            let dir = self.resolve_agent_dir(&entry.directory)?;
            (entry.name, dir)
        };

        if !agent_dir.exists() {
            return Err(format!(
                "Agent directory not found: {}",
                agent_dir.display()
            ));
        }

        let backups_dir = self.data_root.join("backups");
        std::fs::create_dir_all(&backups_dir)
            .map_err(|e| format!("Failed to create backups directory: {}", e))?;

        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let safe_name =
            agent_name.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_");
        let backup_name = format!("{}_{}_backup", timestamp, safe_name);
        let backup_path = backups_dir.join(&backup_name);

        copy_dir_recursive(&agent_dir, &backup_path)
            .map_err(|e| format!("Failed to copy agent directory: {}", e))?;

        // Write backup metadata
        let metadata = BackupMetadata {
            agent_id: agent_id.to_string(),
            agent_name: agent_name.clone(),
            backup_type: "manual_backup".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            source_directory: agent_dir.to_string_lossy().to_string(),
        };
        let metadata_path = backup_path.join("backup_metadata.json");
        if let Ok(json) = serde_json::to_string_pretty(&metadata) {
            let _ = std::fs::write(&metadata_path, json);
        }

        tracing::info!("Backed up agent {} to {}", agent_id, backup_path.display());
        Ok(backup_path.to_string_lossy().to_string())
    }

    /// List all backups in `{data_root}/backups/`.
    pub fn list_backups(&self) -> Result<Vec<BackupInfo>, String> {
        let backups_dir = self.data_root.join("backups");
        if !backups_dir.exists() {
            return Ok(Vec::new());
        }

        let mut backups = Vec::new();
        let entries = std::fs::read_dir(&backups_dir)
            .map_err(|e| format!("Failed to read backups: {}", e))?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = entry.file_name().to_string_lossy().to_string();

            // Try reading backup_metadata.json first
            let metadata_path = path.join("backup_metadata.json");
            let (agent_name, backup_type, created_at) = if metadata_path.exists() {
                match std::fs::read_to_string(&metadata_path) {
                    Ok(json) => match serde_json::from_str::<BackupMetadata>(&json) {
                        Ok(m) => (m.agent_name, m.backup_type, m.created_at),
                        Err(_) => parse_backup_dir_name(&dir_name),
                    },
                    Err(_) => parse_backup_dir_name(&dir_name),
                }
            } else {
                parse_backup_dir_name(&dir_name)
            };

            // Read config.json for birth status
            let config_path = path.join("config.json");
            let (birth_complete, birth_date) = if config_path.exists() {
                match AppConfig::load(&config_path) {
                    Ok(config) => (config.birth_complete, config.birth_timestamp.clone()),
                    Err(_) => (false, None),
                }
            } else {
                (false, None)
            };

            // Check for memory databases (both naming conventions)
            let has_memories =
                path.join("abigail_seed.db").exists() || path.join("abigail_memory.db").exists();

            // Check for signatures
            let has_signatures = path.join("docs").join("soul.md.sig").exists();

            backups.push(BackupInfo {
                directory_name: dir_name,
                directory_path: path.to_string_lossy().to_string(),
                agent_name,
                backup_type,
                created_at,
                birth_complete,
                birth_date,
                has_memories,
                has_signatures,
            });
        }

        // Sort newest first
        backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(backups)
    }

    /// Restore an entity from a backup directory.
    /// Creates a new UUID, moves the backup into identities/, updates paths,
    /// re-signs with the current master key, and registers in GlobalConfig.
    pub fn restore_backup(&self, backup_dir_name: &str) -> Result<String, String> {
        let backups_dir = self.data_root.join("backups");
        let backup_path = backups_dir.join(backup_dir_name);

        if !backup_path.exists() {
            return Err(format!("Backup not found: {}", backup_dir_name));
        }

        // Read config to get agent name
        let config_path = backup_path.join("config.json");
        let agent_name = if config_path.exists() {
            match AppConfig::load(&config_path) {
                Ok(config) => config
                    .agent_name
                    .unwrap_or_else(|| "Restored Entity".to_string()),
                Err(_) => "Restored Entity".to_string(),
            }
        } else {
            "Restored Entity".to_string()
        };

        // Generate new UUID to avoid conflicts
        let new_uuid = Uuid::new_v4().to_string();
        let agent_dir = self.identities_dir().join(&new_uuid);

        // Move backup to identities dir (with copy+delete fallback for cross-device)
        if std::fs::rename(&backup_path, &agent_dir).is_err() {
            copy_dir_recursive(&backup_path, &agent_dir)
                .map_err(|e| format!("Failed to copy backup to identities: {}", e))?;
            std::fs::remove_dir_all(&backup_path)
                .map_err(|e| format!("Backup copied but failed to remove original: {}", e))?;
        }

        // Update config.json paths
        let new_config_path = agent_dir.join("config.json");
        if new_config_path.exists() {
            match AppConfig::load(&new_config_path) {
                Ok(mut config) => {
                    config.data_dir = agent_dir.clone();
                    config.models_dir = agent_dir.join("models");
                    config.docs_dir = agent_dir.join("docs");
                    config.db_path = if agent_dir.join("abigail_memory.db").exists() {
                        agent_dir.join("abigail_memory.db")
                    } else {
                        agent_dir.join("abigail_seed.db")
                    };
                    if agent_dir.join("external_pubkey.bin").exists() {
                        config.external_pubkey_path = Some(agent_dir.join("external_pubkey.bin"));
                    }
                    let _ = config.save(&new_config_path);
                }
                Err(e) => {
                    tracing::warn!("Failed to update restored config: {}", e);
                }
            }
        }

        // Re-sign agent public key with current Hive master key
        let agent_pubkey_path = agent_dir.join("external_pubkey.bin");
        if agent_pubkey_path.exists() {
            let pubkey_bytes = std::fs::read(&agent_pubkey_path).map_err(|e| e.to_string())?;
            if pubkey_bytes.len() == 32 {
                let pubkey_array: [u8; 32] = pubkey_bytes.as_slice().try_into().unwrap();
                if let Ok(agent_pubkey) = VerifyingKey::from_bytes(&pubkey_array) {
                    let signature = sign_agent_key(&self.master_key, &agent_pubkey);
                    let sig_path = agent_dir.join("signature.sig");
                    std::fs::write(&sig_path, &signature).map_err(|e| e.to_string())?;
                }
            }
        }

        // Remove leftover backup_metadata.json
        let _ = std::fs::remove_file(agent_dir.join("backup_metadata.json"));

        // Register in GlobalConfig
        {
            let mut gc = self.global_config.write().map_err(|e| e.to_string())?;
            gc.register_agent(AgentEntry {
                id: new_uuid.clone(),
                name: agent_name.clone(),
                directory: self.registry_directory_for(&new_uuid),
            })
            .map_err(|e| e.to_string())?;
            gc.save(&self.data_root).map_err(|e| e.to_string())?;
        }

        // Create Documents folder
        let _ = self.create_documents_folder(&new_uuid);

        tracing::info!(
            "Restored backup '{}' as agent {} ({})",
            backup_dir_name,
            agent_name,
            new_uuid
        );
        Ok(new_uuid)
    }

    /// Delete a backup directory.
    pub fn delete_backup(&self, backup_dir_name: &str) -> Result<(), String> {
        let backups_dir = self.data_root.join("backups");
        let backup_path = backups_dir.join(backup_dir_name);

        if !backup_path.exists() {
            return Err(format!("Backup not found: {}", backup_dir_name));
        }

        std::fs::remove_dir_all(&backup_path)
            .map_err(|e| format!("Failed to delete backup: {}", e))?;

        tracing::info!("Deleted backup: {}", backup_dir_name);
        Ok(())
    }
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Parse a backup directory name into (agent_name, backup_type, created_at).
/// Expected formats: `YYYYMMDD_HHMMSS_Name` (archive) or `YYYYMMDD_HHMMSS_Name_backup` (backup).
fn parse_backup_dir_name(dir_name: &str) -> (String, String, String) {
    let parts: Vec<&str> = dir_name.splitn(3, '_').collect();
    if parts.len() >= 3 {
        let date_part = parts[0]; // YYYYMMDD
        let time_part = parts[1]; // HHMMSS
        let rest = parts[2];

        // Build an approximate ISO timestamp
        let created_at = if date_part.len() == 8 && time_part.len() == 6 {
            format!(
                "{}-{}-{}T{}:{}:{}Z",
                &date_part[..4],
                &date_part[4..6],
                &date_part[6..8],
                &time_part[..2],
                &time_part[2..4],
                &time_part[4..6]
            )
        } else {
            String::new()
        };

        let (name, backup_type) = if rest.ends_with("_backup") {
            (
                rest.trim_end_matches("_backup").to_string(),
                "manual_backup".to_string(),
            )
        } else {
            (rest.to_string(), "archive".to_string())
        };

        (name, backup_type, created_at)
    } else {
        (dir_name.to_string(), "unknown".to_string(), String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manager() -> IdentityManager {
        let tmp = std::env::temp_dir().join(format!("abigail_identity_test_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        IdentityManager {
            data_root: tmp.clone(),
            global_config: RwLock::new(GlobalConfig::new(&tmp)),
            master_key: SigningKey::from_bytes(&[9u8; 32]),
        }
    }

    #[test]
    fn resolve_agent_dir_rejects_absolute_registry_paths() {
        let manager = test_manager();
        let absolute = std::env::temp_dir().join("identities/absolute-agent");
        assert!(manager.resolve_agent_dir(&absolute).is_err());
        let _ = std::fs::remove_dir_all(manager.data_root());
    }

    #[test]
    fn resolve_agent_dir_accepts_relative_registry_paths() {
        let manager = test_manager();
        let resolved = manager
            .resolve_agent_dir(Path::new("identities/test-agent"))
            .unwrap();
        assert_eq!(resolved, manager.data_root().join("identities/test-agent"));
        let _ = std::fs::remove_dir_all(manager.data_root());
    }

    #[test]
    fn migrate_legacy_identity_copies_current_vault_and_db_files() {
        let tmp = std::env::temp_dir().join(format!(
            "abigail_identity_migrate_legacy_{}",
            Uuid::new_v4()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("docs")).unwrap();

        let manager = IdentityManager {
            data_root: tmp.clone(),
            global_config: RwLock::new(GlobalConfig::new(&tmp)),
            master_key: SigningKey::from_bytes(&[9u8; 32]),
        };

        let mut legacy_config = AppConfig::default_paths();
        legacy_config.data_dir = tmp.clone();
        legacy_config.models_dir = tmp.join("models");
        legacy_config.docs_dir = tmp.join("docs");
        legacy_config.db_path = tmp.join("abigail_memory.db");
        legacy_config.birth_complete = true;
        legacy_config.agent_name = Some("Legacy Agent".to_string());
        legacy_config.external_pubkey_path = Some(tmp.join("external_pubkey.bin"));
        legacy_config.save(&tmp.join("config.json")).unwrap();

        std::fs::write(tmp.join("abigail_memory.db"), b"memory").unwrap();
        std::fs::write(tmp.join("abigail_memory.db-wal"), b"wal").unwrap();
        std::fs::write(tmp.join("jobs.db"), b"jobs").unwrap();
        std::fs::write(tmp.join("secrets.vault"), b"secrets").unwrap();
        std::fs::write(tmp.join("skills.vault"), b"skills").unwrap();
        std::fs::write(tmp.join("keys.vault"), b"keys").unwrap();
        std::fs::write(tmp.join("vault.sentinel"), b"sentinel").unwrap();
        std::fs::write(tmp.join("docs").join("soul.md"), b"legacy soul").unwrap();

        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        std::fs::write(
            tmp.join("external_pubkey.bin"),
            signing_key.verifying_key().to_bytes(),
        )
        .unwrap();

        let agent_id = manager.migrate_legacy_identity().unwrap().unwrap();
        let agent_dir = manager.agent_dir(&agent_id).unwrap();

        assert!(agent_dir.join("abigail_memory.db").exists());
        assert!(agent_dir.join("abigail_memory.db-wal").exists());
        assert!(agent_dir.join("jobs.db").exists());
        assert!(agent_dir.join("secrets.vault").exists());
        assert!(agent_dir.join("skills.vault").exists());
        assert!(agent_dir.join("keys.vault").exists());
        assert!(agent_dir.join("vault.sentinel").exists());
        assert!(agent_dir.join("docs").join("soul.md").exists());
        assert!(agent_dir.join("signature.sig").exists());

        let migrated_config = AppConfig::load(&agent_dir.join("config.json")).unwrap();
        assert_eq!(migrated_config.data_dir, agent_dir);
        assert_eq!(migrated_config.db_path, agent_dir.join("abigail_memory.db"));
        assert_eq!(
            migrated_config.external_pubkey_path,
            Some(agent_dir.join("external_pubkey.bin"))
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
