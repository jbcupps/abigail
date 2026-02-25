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
    /// Create a new IdentityManager, loading GlobalConfig and master key from disk.
    /// If master key doesn't exist, generates one (first-run bootstrap).
    pub fn new(data_root: PathBuf) -> anyhow::Result<Self> {
        // Ensure directories exist
        std::fs::create_dir_all(&data_root)?;
        let identities_dir = data_root.join("identities");
        std::fs::create_dir_all(&identities_dir)?;

        // Load or create master key
        let master_key_path = data_root.join("master.key");
        let master_key = if master_key_path.exists() {
            load_master_key(&master_key_path)?
        } else {
            tracing::info!("No master key found, generating new Hive master key");
            generate_master_key(&data_root)?;
            load_master_key(&master_key_path)?
        };

        // Load or create global config
        let global_config = if GlobalConfig::config_path(&data_root).exists() {
            GlobalConfig::load(&data_root)?
        } else {
            let config = GlobalConfig::new(&data_root);
            config.save(&data_root)?;
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

    /// Get the identities directory path.
    pub fn identities_dir(&self) -> PathBuf {
        self.data_root.join("identities")
    }

    /// List all registered agents with their info.
    pub fn list_agents(&self) -> Result<Vec<AgentIdentityInfo>, String> {
        let gc = self.global_config.read().map_err(|e| e.to_string())?;
        let mut agents = Vec::new();

        for entry in &gc.agents {
            let agent_dir = if entry.directory.is_absolute() {
                entry.directory.clone()
            } else {
                self.data_root.join(&entry.directory)
            };

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

        let agent_dir = if entry.directory.is_absolute() {
            entry.directory.clone()
        } else {
            self.data_root.join(&entry.directory)
        };

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

        let agent_dir = if entry.directory.is_absolute() {
            entry.directory.clone()
        } else {
            self.data_root.join(&entry.directory)
        };

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
            tier_models: None,
            force_override: Default::default(),
            tier_thresholds: Default::default(),
            provider_catalog: Vec::new(),
            active_provider_preference: None,
            email_accounts: Vec::new(),
            bundled_ollama: cfg!(windows),
            bundled_model: Some("qwen2.5:0.5b".to_string()),
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
        };
        let config_path = agent_dir.join("config.json");
        config.save(&config_path).map_err(|e| e.to_string())?;

        // Register in global config
        {
            let mut gc = self.global_config.write().map_err(|e| e.to_string())?;
            gc.register_agent(AgentEntry {
                id: uuid.clone(),
                name: name.to_string(),
                directory: PathBuf::from(format!("identities/{}", uuid)),
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

        let agent_dir = if entry.directory.is_absolute() {
            entry.directory.clone()
        } else {
            self.data_root.join(&entry.directory)
        };
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

        let agent_dir = if entry.directory.is_absolute() {
            entry.directory.clone()
        } else {
            self.data_root.join(&entry.directory)
        };
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

        // Move legacy files to agent directory
        let files_to_move = [
            "config.json",
            "keys.bin",
            "external_pubkey.bin",
            "secrets.bin",
            "abigail_seed.db",
            "abigail_seed.db-wal",
            "abigail_seed.db-shm",
        ];

        for file in &files_to_move {
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
            config.db_path = agent_dir.join("abigail_seed.db");
            if legacy_pubkey.exists() {
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
                directory: PathBuf::from(format!("identities/{}", uuid)),
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

            let dir = if entry.directory.is_absolute() {
                entry.directory.clone()
            } else {
                self.data_root.join(&entry.directory)
            };

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

            let dir = if entry.directory.is_absolute() {
                entry.directory.clone()
            } else {
                self.data_root.join(&entry.directory)
            };

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

        Ok(backup_path.to_string_lossy().to_string())
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
