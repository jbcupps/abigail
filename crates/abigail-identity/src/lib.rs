//! Hive identity management — thread-safe manager for multi-agent identities.
//!
//! Extracted from `tauri-app/src/identity_manager.rs` as a standalone crate.
//! Zero Tauri imports — only uses `abigail_core`, `ed25519_dalek`, `serde`, `uuid`, `std`.

use abigail_core::{
    encrypted_storage, generate_master_key, load_master_key, sign_agent_key,
    verify_agent_signature, AgentEntry, AppConfig, GlobalConfig, SecretsVault,
};
use ed25519_dalek::{SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

mod creation;
mod hive;

pub use creation::{AgentCreationError, AgentCreationStatus};
pub use hive::{HiveEntity, HIVE_ENTITY_NAME};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecoveryBundle {
    format: String,
    agent_id: String,
    agent_name: Option<String>,
    created_at: String,
    private_key_base64: String,
}

/// Information about an agent identity for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentityInfo {
    pub id: String,
    pub name: String,
    pub directory: String,
    pub birth_complete: bool,
    pub birth_date: Option<String>,
    pub is_hive: bool,
    pub immortal: bool,
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
    profile: Option<Arc<abigail_core::profile::ProfileContext>>,
    global_config: RwLock<GlobalConfig>,
    master_key: SigningKey,
}

impl IdentityManager {
    /// A missing key in an established preview is a recovery condition, not
    /// permission to generate a replacement that cannot verify its Entities.
    /// Run before credential unlock and repeat before any bootstrap writes.
    fn validate_preview_bootstrap(data_root: &Path, preview: bool) -> Result<(), String> {
        if !preview {
            return Ok(());
        }
        if data_root
            .join("master.key")
            .try_exists()
            .map_err(|e| e.to_string())?
        {
            return Ok(());
        }
        let has_registry = GlobalConfig::config_path(data_root)
            .try_exists()
            .map_err(|e| e.to_string())?;
        let identities = data_root.join("identities");
        let has_identities = if identities.try_exists().map_err(|e| e.to_string())? {
            std::fs::read_dir(&identities)
                .map_err(|e| e.to_string())?
                .next()
                .transpose()
                .map_err(|e| e.to_string())?
                .is_some()
        } else {
            false
        };
        if has_registry || has_identities {
            return Err(
                "Preview identity recovery is required: master.key is missing from an existing identity profile. Refusing to generate a replacement key."
                    .into(),
            );
        }
        Ok(())
    }

    fn should_auto_reset_identity_state(error: &str) -> bool {
        let error = error.to_ascii_lowercase();
        error.contains("aes-gcm decryption failed")
            || error.contains("wrong key or tampered data")
            || error.contains("decryption failed")
    }

    /// Keep recovery dispatch injectable so preview refusal can be checked
    /// without unlocking a real vault or bootstrapping an identity.
    fn load_with_recovery<T>(
        preview: bool,
        mut load: impl FnMut() -> Result<T, String>,
        recover: impl FnOnce() -> Result<(), String>,
    ) -> anyhow::Result<T> {
        match load() {
            Ok(value) => Ok(value),
            Err(error) if !preview && Self::should_auto_reset_identity_state(&error) => {
                tracing::warn!(
                    error = %error,
                    "AES-GCM decryption failed while loading identity state; archiving identity material and bootstrapping fresh state"
                );
                recover().map_err(anyhow::Error::msg)?;
                load().map_err(|retry_error| {
                    tracing::error!("Fatal identity error after auto-recovery: {}", retry_error);
                    anyhow::anyhow!(retry_error)
                })
            }
            Err(error) => {
                tracing::error!("Fatal identity error: {}", error);
                if preview && Self::should_auto_reset_identity_state(&error) {
                    return Err(anyhow::anyhow!(
                        "Preview identity recovery requires an explicit restore; automatic reset is disabled: {}",
                        error
                    ));
                }
                Err(anyhow::anyhow!(error))
            }
        }
    }

    fn recover_identity_state(data_root: &Path) -> Result<(), String> {
        let candidates = [
            data_root.join("master.key"),
            GlobalConfig::config_path(data_root),
            data_root.join("identities"),
        ];

        if !candidates.iter().any(|path| path.exists()) {
            return Ok(());
        }

        let backups_dir = data_root.join("backups");
        std::fs::create_dir_all(&backups_dir).map_err(|e| e.to_string())?;

        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let recovery_dir = backups_dir.join(format!("identity_recovery_{}", timestamp));
        std::fs::create_dir_all(&recovery_dir).map_err(|e| e.to_string())?;

        for path in candidates {
            if !path.exists() {
                continue;
            }

            let file_name = path
                .file_name()
                .ok_or_else(|| format!("Invalid identity recovery path '{}'", path.display()))?;
            let target = recovery_dir.join(file_name);
            std::fs::rename(&path, &target).map_err(|e| {
                format!(
                    "Failed to archive '{}' during identity recovery: {}",
                    path.display(),
                    e
                )
            })?;
        }

        tracing::warn!(
            recovery_dir = %recovery_dir.display(),
            "Archived identity state after AES-GCM failure; shared Hive memory/docs remain untouched"
        );

        Ok(())
    }

    fn ensure_hive_entity(
        data_root: &Path,
        global_config: &mut GlobalConfig,
    ) -> Result<(), String> {
        if global_config.agents.iter().any(|entry| entry.is_hive) {
            return Ok(());
        }

        let hive = HiveEntity::new(Uuid::new_v4().to_string());
        let agent_dir = data_root.join("identities").join(&hive.id);
        std::fs::create_dir_all(agent_dir.join("docs")).map_err(|e| e.to_string())?;

        let config = hive.config(data_root, &global_config.default_theme);
        config
            .save(&agent_dir.join("config.json"))
            .map_err(|e| e.to_string())?;

        global_config
            .register_agent(hive.registry_entry())
            .map_err(|e| e.to_string())?;
        global_config.save(data_root).map_err(|e| e.to_string())?;

        tracing::info!(
            "Provisioned '{}' with locked entitlements {:?}",
            HIVE_ENTITY_NAME,
            HiveEntity::locked_entitlements()
        );

        Ok(())
    }

    fn normalize_agent_directory(&self, directory: &Path) -> Result<PathBuf, String> {
        abigail_core::global_config::normalize_agent_directory(directory).map_err(|e| e.to_string())
    }

    fn resolve_agent_dir(&self, directory: &Path) -> Result<PathBuf, String> {
        let relative = self.normalize_agent_directory(directory)?;
        let path = abigail_core::path_guard::resolve_within_root(
            &self.data_root,
            &relative,
            "agent directory",
        )
        .map_err(|e| e.to_string())?;
        self.validate_profile_path(&path)?;
        Ok(path)
    }

    fn validate_profile_path(&self, path: &Path) -> Result<(), String> {
        if let Some(profile) = &self.profile {
            profile.validate_path(path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn validate_profile_config(&self, config: &AppConfig, path: &Path) -> Result<(), String> {
        if let Some(profile) = &self.profile {
            profile
                .validate_config_at_path(config, path)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn read_compatible_config(&self, path: &Path) -> Result<AppConfig, String> {
        self.validate_profile_path(path)?;
        abigail_core::path_guard::ensure_expected_filename(path, "config.json")
            .map_err(|e| e.to_string())?;
        let parent = path.parent().ok_or("Config path has no parent directory")?;
        let content = abigail_core::path_guard::load_string_from_root(parent, "config.json")
            .map_err(|e| e.to_string())?;
        let config = AppConfig::from_json_compatible(&content).map_err(|e| e.to_string())?;
        self.validate_profile_config(&config, path)?;
        Ok(config)
    }

    fn load_config(&self, path: &Path) -> Result<AppConfig, String> {
        if self.profile.is_none() {
            return AppConfig::load(path).map_err(|e| e.to_string());
        }
        // Validate the value that will be used before migration can write it back.
        let mut config = self.read_compatible_config(path)?;
        if config.migrate() {
            config.save(path).map_err(|e| e.to_string())?;
        }
        Ok(config)
    }

    fn backups_dir(&self) -> Result<PathBuf, String> {
        let path = self.profile.as_ref().map_or_else(
            || self.data_root.join("backups"),
            |profile| profile.backups_dir(),
        );
        self.validate_profile_path(&path)?;
        Ok(path)
    }

    fn backup_path(&self, directory_name: &str) -> Result<PathBuf, String> {
        if self.profile.is_some() {
            let relative = Path::new(directory_name);
            abigail_core::path_guard::ensure_relative_no_traversal(relative, "backup directory")
                .map_err(|e| e.to_string())?;
            if relative.components().count() != 1 || directory_name == "." {
                return Err("Backup directory must be a single directory name.".to_string());
            }
        }
        let path = self.backups_dir()?.join(directory_name);
        self.validate_profile_path(&path)?;
        Ok(path)
    }

    fn validate_profile_tree(&self, path: &Path) -> Result<(), String> {
        if self.profile.is_none() {
            return Ok(());
        }
        self.validate_profile_path(path)?;
        if path.is_dir() {
            for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                self.validate_profile_tree(&entry.path())?;
            }
        }
        Ok(())
    }

    fn registry_directory_for(&self, agent_id: &str) -> PathBuf {
        PathBuf::from("identities").join(agent_id)
    }

    fn shared_memory_db_path(&self) -> PathBuf {
        self.profile.as_ref().map_or_else(
            || HiveEntity::memory_db_path(&self.data_root),
            |profile| profile.shared_memory_path(),
        )
    }

    /// Create a new IdentityManager, loading GlobalConfig and master key from disk.
    /// If master key doesn't exist, generates one (first-run bootstrap).
    pub fn new(data_root: PathBuf) -> anyhow::Result<Self> {
        let profile = abigail_core::profile::current()?;
        let data_root = if let Some(profile) = &profile {
            profile.validate_path(&data_root)?;
            if !abigail_core::profile::same_path(
                &abigail_core::profile::canonical_without_links(&data_root)?,
                profile.root(),
            ) {
                anyhow::bail!("Identity data root does not match the validated preview profile.");
            }
            profile.root().to_path_buf()
        } else {
            data_root
        };
        Self::validate_preview_bootstrap(&data_root, profile.is_some())
            .map_err(anyhow::Error::msg)?;
        // paper Sections 22-27 runtime verification:
        // the KEK must already be session-verified before identity bootstrap.
        let unlock = abigail_core::HybridUnlockProvider::new();
        use abigail_core::UnlockProvider as _;
        unlock
            .root_kek()
            .map_err(|e| anyhow::anyhow!(format!("Recovery Mode: {}", e)))?;

        Self::load_with_recovery(
            profile.is_some(),
            || Self::try_load_from_vault(data_root.clone(), profile.clone()),
            || {
                tracing::warn!(
                    data_root = %data_root.display(),
                    "Recovering stable identity state"
                );
                Self::recover_identity_state(&data_root)
            },
        )
    }

    fn try_load_from_vault(
        data_root: PathBuf,
        profile: Option<Arc<abigail_core::profile::ProfileContext>>,
    ) -> Result<Self, String> {
        Self::validate_preview_bootstrap(&data_root, profile.is_some())?;
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
        let mut global_config = global_config;
        Self::ensure_hive_entity(&data_root, &mut global_config)?;

        Ok(Self {
            data_root,
            profile,
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
        self.profile.as_ref().map_or_else(
            || self.data_root.join("identities"),
            |profile| profile.identities_dir(),
        )
    }

    pub fn hive_agent_id(&self) -> Result<String, String> {
        let gc = self.global_config.read().map_err(|e| e.to_string())?;
        gc.agents
            .iter()
            .find(|entry| entry.is_hive)
            .map(|entry| entry.id.clone())
            .ok_or_else(|| "Abigail Hive is not registered".to_string())
    }

    /// List all registered agents with their info.
    pub fn list_agents(&self) -> Result<Vec<AgentIdentityInfo>, String> {
        let gc = self.global_config.read().map_err(|e| e.to_string())?;
        let mut agents = Vec::new();

        for entry in &gc.agents {
            let agent_dir = self.resolve_agent_dir(&entry.directory)?;

            let config_path = agent_dir.join("config.json");
            let (birth_complete, birth_date) = if config_path.exists() {
                match self.load_config(&config_path) {
                    Ok(config) => (config.birth_complete, config.birth_timestamp.clone()),
                    Err(error) if self.profile.is_some() => return Err(error),
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
                is_hive: entry.is_hive,
                immortal: entry.is_hive,
            });
        }

        agents.sort_by(|left, right| {
            right
                .is_hive
                .cmp(&left.is_hive)
                .then_with(|| left.name.cmp(&right.name))
        });

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

        let config = self.load_config(&config_path)?;
        // Both verification and Documents setup acquire the registry again.
        // Never recursively hold a read guard: a waiting creator needs the
        // write lock, and writer-prioritizing RwLocks then block the next read.
        drop(gc);

        // Only verify signature for born agents (unborn agents don't have keys yet)
        if config.birth_complete && !config.is_hive {
            self.verify_agent(agent_id)?;
        } else {
            tracing::info!("Skipping signature verification for entity {}", agent_id);
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
        let mut gc = self.global_config.write().map_err(|e| e.to_string())?;
        let agent_dir = self.identities_dir().join(&uuid);
        if gc
            .creation_attempts
            .values()
            .any(|attempt| attempt.entity_id == uuid)
            || agent_dir.try_exists().map_err(|e| e.to_string())?
        {
            return Err("New entity directory already exists.".to_string());
        }
        self.initialize_agent_files(&uuid, name, &gc.default_theme)?;

        // Publish in memory only after the atomic registry write succeeds.
        let mut next = gc.clone();
        next.register_agent(AgentEntry {
            id: uuid.clone(),
            name: name.to_string(),
            is_hive: false,
            directory: self.registry_directory_for(&uuid),
        })
        .map_err(|e| e.to_string())?;
        next.save(&self.data_root).map_err(|e| e.to_string())?;
        *gc = next;
        drop(gc);
        let _ = self.create_documents_folder(&uuid);
        tracing::info!("Created new agent: {} ({})", name, uuid);
        Ok((uuid, agent_dir))
    }

    /// Initialize a reserved directory, preserving an existing valid config on replay.
    fn initialize_agent_files(&self, uuid: &str, name: &str, theme: &str) -> Result<(), String> {
        let agent_dir = self.identities_dir().join(uuid);
        self.validate_profile_path(&agent_dir)?;
        let config_path = agent_dir.join("config.json");
        if config_path.try_exists().map_err(|e| e.to_string())? {
            // Validate compatibility without AppConfig::load's migration/write side effect.
            let existing = self.read_compatible_config(&config_path)?;
            if existing.is_hive
                || existing.data_dir != agent_dir
                || existing.agent_name.as_deref() != Some(name)
            {
                return Err(
                    "Reserved entity config does not match the creation attempt.".to_string(),
                );
            }
            return Ok(());
        }

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
            db_path: self.shared_memory_db_path(),
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
            is_hive: false,
            mcp_servers: Default::default(),
            mcp_trust_policy: Default::default(),
            approved_skill_ids: Default::default(),
            trusted_skill_signers: Default::default(),
            sao_endpoint: None,
            provider_catalog: Vec::new(),
            active_provider_preference: None,
            ego_model: None,
            model_routing: None,
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
            hive_daemon_url: "http://127.0.0.1:43141".to_string(),
            entity_daemon_url: "http://127.0.0.1:43142".to_string(),
            iggy_connection: None,
            theme_id: Some(theme.to_string()),
            autonomy_profile: Default::default(),
        };
        config.save(&config_path).map_err(|e| e.to_string())?;
        Ok(())
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
        abigail_core::secure_fs::write_bytes_atomic(&sig_path, &signature)
            .map_err(|e| e.to_string())?;

        tracing::info!(
            "Signed agent {} with Hive master key (post-birth)",
            agent_id
        );
        Ok(())
    }

    /// Complete the local identity trust chain before marking an Entity born.
    /// The signing key stays in an encrypted recovery file so an explicit
    /// later birth rite can re-sign documents without replacing the identity.
    pub fn sign_birth_documents(&self, agent_id: &str) -> Result<PathBuf, String> {
        let agent_dir = self.agent_dir(agent_id)?;
        let private_path = agent_dir.join("birth_signing_key.vault");
        let public_path = agent_dir.join("external_pubkey.bin");
        let private_key = if private_path.exists() {
            String::from_utf8(
                encrypted_storage::read_encrypted(&private_path).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?
        } else {
            if public_path.exists() {
                return Err(
                    "This Entity's recovery signing key is required to change its constitution."
                        .to_string(),
                );
            }
            let generated =
                abigail_core::generate_external_keypair(&agent_dir).map_err(|e| e.to_string())?;
            if let Err(error) = encrypted_storage::write_encrypted(
                &private_path,
                generated.private_key_base64.as_bytes(),
            ) {
                let _ = std::fs::remove_file(&public_path);
                return Err(error.to_string());
            }
            generated.private_key_base64
        };
        let signing_key =
            abigail_core::parse_private_key(&private_key).map_err(|e| e.to_string())?;
        let expected_public = signing_key.verifying_key().to_bytes();
        if public_path.exists() {
            let actual_public = std::fs::read(&public_path).map_err(|e| e.to_string())?;
            if actual_public != expected_public {
                return Err(
                    "Entity signing key does not match its registered public key.".to_string(),
                );
            }
        } else {
            abigail_core::secure_fs::write_bytes_atomic(&public_path, &expected_public)
                .map_err(|e| e.to_string())?;
        }
        abigail_core::sign_constitutional_documents(&signing_key, &agent_dir.join("docs"))
            .map_err(|e| e.to_string())?;
        self.sign_agent_after_birth(agent_id)?;
        Ok(public_path)
    }

    /// Sign an arbitrary payload with the Hive master key.
    ///
    /// Returns `(signature_hex, master_public_key_hex)`. Used for artifacts
    /// the Hive vouches for, like birth certificates.
    pub fn sign_payload(&self, payload: &[u8]) -> (String, String) {
        use ed25519_dalek::Signer as _;
        let signature = self.master_key.sign(payload);
        let to_hex =
            |bytes: &[u8]| -> String { bytes.iter().map(|b| format!("{:02x}", b)).collect() };
        (
            to_hex(&signature.to_bytes()),
            to_hex(&self.master_key.verifying_key().to_bytes()),
        )
    }

    /// Verify an artifact against this Hive's master key, never a key supplied
    /// by the artifact itself.
    pub fn verify_payload(
        &self,
        payload: &[u8],
        signature_hex: &str,
        public_key_hex: &str,
    ) -> Result<(), String> {
        use ed25519_dalek::Verifier as _;
        let public_key = decode_hex(public_key_hex)?;
        if public_key != self.master_key.verifying_key().to_bytes() {
            return Err("Artifact was not issued by this Hive.".to_string());
        }
        let signature_bytes = decode_hex(signature_hex)?;
        let signature =
            ed25519_dalek::Signature::from_slice(&signature_bytes).map_err(|e| e.to_string())?;
        self.master_key
            .verifying_key()
            .verify(payload, &signature)
            .map_err(|_| "Artifact signature verification failed.".to_string())
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
            if entry.is_hive {
                return Err("Abigail Hive is immortal and its identity is locked.".to_string());
            }
            entry.name = new_name.to_string();
            gc.save(&self.data_root).map_err(|e| e.to_string())?;
            Ok(())
        } else {
            Err(format!("Agent {} not registered", agent_id))
        }
    }

    /// Create (or ensure existence of) the per-agent Documents folder.
    /// Normal installs use the user's `Documents/Abigail/{agent_name}`.
    /// Custom data roots keep that tree under their own `Documents`; an
    /// explicit `ABIGAIL_DOCUMENTS_DIR` overrides the Documents base.
    /// Validated preview profiles use UUID directories and never consult that override.
    pub fn create_documents_folder(&self, agent_id: &str) -> Result<PathBuf, String> {
        let agent_name = {
            let gc = self.global_config.read().map_err(|e| e.to_string())?;
            let entry = gc
                .find_agent(agent_id)
                .ok_or_else(|| format!("Agent {} not registered", agent_id))?;
            entry.name.clone()
        };

        if let Some(profile) = &self.profile {
            let documents = profile
                .entity_documents(agent_id)
                .map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&documents)
                .map_err(|e| format!("Failed to create preview Documents folder: {}", e))?;
            return Ok(documents);
        }

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

        let isolated_docs = std::env::var_os("ABIGAIL_DOCUMENTS_DIR")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .or_else(|| {
                (cfg!(test) || self.data_root != AppConfig::default_paths().data_dir)
                    .then(|| self.data_root.join("Documents"))
            });
        let docs_base = if let Some(path) = isolated_docs {
            path
        } else {
            #[cfg(windows)]
            {
                let profile = std::env::var("USERPROFILE")
                    .map_err(|_| "USERPROFILE environment variable not set")?;
                PathBuf::from(profile).join("Documents")
            }
            #[cfg(not(windows))]
            {
                let home =
                    std::env::var("HOME").map_err(|_| "HOME environment variable not set")?;
                PathBuf::from(home).join("Documents")
            }
        };

        let agent_docs = docs_base.join("Abigail").join(&safe_name);
        std::fs::create_dir_all(&agent_docs)
            .map_err(|e| format!("Failed to create Documents folder: {}", e))?;

        tracing::info!("Documents folder ready: {}", agent_docs.display());
        Ok(agent_docs)
    }

    /// Save an encrypted recovery bundle in the agent's Documents folder.
    pub fn save_recovery_key(&self, agent_id: &str, private_key: &str) -> Result<String, String> {
        let docs_dir = self.create_documents_folder(agent_id)?;
        let file_path = docs_dir.join("RECOVERY_BUNDLE.abigail-recovery");
        let agent_name = {
            let gc = self.global_config.read().map_err(|e| e.to_string())?;
            gc.find_agent(agent_id).map(|entry| entry.name.clone())
        };
        let bundle = RecoveryBundle {
            format: "abigail-recovery-bundle-v1".to_string(),
            agent_id: agent_id.to_string(),
            agent_name,
            created_at: chrono::Utc::now().to_rfc3339(),
            private_key_base64: private_key.to_string(),
        };
        let content = serde_json::to_vec_pretty(&bundle).map_err(|e| e.to_string())?;
        encrypted_storage::write_encrypted(&file_path, &content).map_err(|e| {
            format!(
                "Failed to write encrypted recovery bundle '{}': {}",
                file_path.display(),
                e
            )
        })?;
        self.append_crypto_audit(
            "recovery_bundle_saved",
            &format!("agent_id={} path={}", agent_id, file_path.display()),
        )?;
        Ok(file_path.to_string_lossy().to_string())
    }

    /// Explicit plaintext export for offline storage and cross-device recovery.
    pub fn save_recovery_key_plaintext(
        &self,
        agent_id: &str,
        private_key: &str,
    ) -> Result<String, String> {
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

        abigail_core::secure_fs::write_string_atomic(&file_path, &content)
            .map_err(|e| format!("Failed to write recovery key file: {}", e))?;
        self.append_crypto_audit(
            "recovery_key_plaintext_saved",
            &format!("agent_id={} path={}", agent_id, file_path.display()),
        )?;

        Ok(file_path.to_string_lossy().to_string())
    }

    fn append_crypto_audit(&self, action: &str, detail: &str) -> Result<(), String> {
        use std::io::Write as _;

        let audit_path = self.data_root.join("crypto_audit.log");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&audit_path)
            .map_err(|e| format!("Failed to open crypto audit log: {}", e))?;
        writeln!(
            file,
            "{} {} {}",
            chrono::Utc::now().to_rfc3339(),
            action,
            detail
        )
        .map_err(|e| format!("Failed to write crypto audit log: {}", e))?;
        Ok(())
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
        if self.profile.is_some() {
            return Err(
                "Preview profiles require explicit identity import; automatic legacy migration is unavailable."
                    .into(),
            );
        }

        let legacy_config = self.load_config(&legacy_config_path)?;

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
            self.validate_profile_path(&src)?;
            self.validate_profile_path(&dst)?;
            if src.exists() {
                std::fs::copy(&src, &dst)
                    .map_err(|e| format!("Failed to copy legacy file {}: {}", file, e))?;
            }
        }

        // Move docs/ directory
        let legacy_docs = self.data_root.join("docs");
        let agent_docs = agent_dir.join("docs");
        if legacy_docs.exists() && !agent_docs.exists() {
            self.validate_profile_tree(&legacy_docs)?;
            copy_dir_recursive(&legacy_docs, &agent_docs)
                .map_err(|e| format!("Failed to copy legacy docs: {}", e))?;
        }

        // Update agent config paths
        let agent_config_path = agent_dir.join("config.json");
        if agent_config_path.exists() {
            let mut config = self.load_config(&agent_config_path)?;
            config.data_dir = agent_dir.clone();
            config.models_dir = agent_dir.join("models");
            config.docs_dir = agent_dir.join("docs");
            config.db_path = self.shared_memory_db_path();
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
                    abigail_core::secure_fs::write_bytes_atomic(&sig_path, &signature)
                        .map_err(|e| e.to_string())?;
                }
            }
        }

        // Register in global config
        {
            let mut gc = self.global_config.write().map_err(|e| e.to_string())?;
            gc.register_agent(AgentEntry {
                id: uuid.clone(),
                name: agent_name.clone(),
                is_hive: false,
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
            if entry.is_hive {
                return Err("Abigail Hive is immortal and cannot be deleted.".to_string());
            }

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
        let backups_dir = self.backups_dir()?;
        let (agent_name, agent_dir) = {
            let mut gc = self.global_config.write().map_err(|e| e.to_string())?;
            let entry = gc
                .find_agent(agent_id)
                .ok_or_else(|| format!("Agent {} not registered", agent_id))?
                .clone();
            if entry.is_hive {
                return Err("Abigail Hive is immortal and cannot be archived.".to_string());
            }

            let dir = self.resolve_agent_dir(&entry.directory)?;
            if self.profile.is_some() {
                self.read_compatible_config(&dir.join("config.json"))?;
                self.validate_profile_tree(&dir)?;
            }

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

        std::fs::create_dir_all(&backups_dir)
            .map_err(|e| format!("Failed to create backups directory: {}", e))?;

        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let safe_name =
            agent_name.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_");
        let backup_name = if self.profile.is_some() {
            format!("{}_{}_{}", timestamp, safe_name, agent_id)
        } else {
            format!("{}_{}", timestamp, safe_name)
        };
        let backup_path = self.backup_path(&backup_name)?;

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

        if self.profile.is_some() {
            self.read_compatible_config(&agent_dir.join("config.json"))?;
            self.validate_profile_tree(&agent_dir)?;
        }
        let backups_dir = self.backups_dir()?;
        std::fs::create_dir_all(&backups_dir)
            .map_err(|e| format!("Failed to create backups directory: {}", e))?;

        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let safe_name =
            agent_name.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_");
        let backup_name = if self.profile.is_some() {
            format!("{}_{}_{}_backup", timestamp, safe_name, Uuid::new_v4())
        } else {
            format!("{}_{}_backup", timestamp, safe_name)
        };
        let backup_path = self.backup_path(&backup_name)?;

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
        let backups_dir = self.backups_dir()?;
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
            self.validate_profile_path(&path)?;
            if !path.is_dir() {
                continue;
            }
            let dir_name = entry.file_name().to_string_lossy().to_string();

            // Try reading backup_metadata.json first
            let metadata_path = path.join("backup_metadata.json");
            self.validate_profile_path(&metadata_path)?;
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
                match self.load_config(&config_path) {
                    Ok(config) => (config.birth_complete, config.birth_timestamp.clone()),
                    Err(error) if self.profile.is_some() => return Err(error),
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
        let backup_path = self.backup_path(backup_dir_name)?;

        if !backup_path.exists() {
            return Err(format!("Backup not found: {}", backup_dir_name));
        }
        self.validate_profile_tree(&backup_path)?;

        // Refuse unsupported or invalid configs before moving or changing the backup.
        // Keep the validated value so loading cannot silently fall back after the move.
        let config_path = backup_path.join("config.json");
        let backup_config = if config_path.try_exists().map_err(|e| e.to_string())? {
            Some(
                self.read_compatible_config(&config_path)
                    .map_err(|e| format!("Cannot restore backup config: {}", e))?,
            )
        } else {
            None
        };
        let agent_name = backup_config
            .as_ref()
            .and_then(|config| config.agent_name.clone())
            .unwrap_or_else(|| "Restored Entity".to_string());

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

        // Migrate supported configs in memory and update the restored paths.
        let new_config_path = agent_dir.join("config.json");
        if let Some(mut config) = backup_config {
            config.migrate();
            config.data_dir = agent_dir.clone();
            config.models_dir = agent_dir.join("models");
            config.docs_dir = agent_dir.join("docs");
            config.db_path = self.shared_memory_db_path();
            if agent_dir.join("external_pubkey.bin").exists()
                || (self.profile.is_some() && config.external_pubkey_path.is_some())
            {
                config.external_pubkey_path = Some(agent_dir.join("external_pubkey.bin"));
            }
            // Preserve existing post-move behavior; transactional restore is separate work.
            let _ = config.save(&new_config_path);
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
                    abigail_core::secure_fs::write_bytes_atomic(&sig_path, &signature)
                        .map_err(|e| e.to_string())?;
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
                is_hive: false,
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
        let backup_path = self.backup_path(backup_dir_name)?;

        if !backup_path.exists() {
            return Err(format!("Backup not found: {}", backup_dir_name));
        }

        std::fs::remove_dir_all(&backup_path)
            .map_err(|e| format!("Failed to delete backup: {}", e))?;

        tracing::info!("Deleted backup: {}", backup_dir_name);
        Ok(())
    }
}

fn decode_hex(input: &str) -> Result<Vec<u8>, String> {
    if !input.is_ascii() || !input.len().is_multiple_of(2) {
        return Err("Invalid hexadecimal signature encoding.".to_string());
    }
    input
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).map_err(|e| e.to_string())?;
            u8::from_str_radix(pair, 16).map_err(|e| e.to_string())
        })
        .collect()
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
mod preview_profile_tests {
    use super::*;

    fn snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, Option<Vec<u8>>> {
        let mut result = std::collections::BTreeMap::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let relative = path.strip_prefix(root).unwrap().to_path_buf();
                if path.is_dir() {
                    result.insert(relative, None);
                    pending.push(path);
                } else {
                    result.insert(relative, Some(std::fs::read(path).unwrap()));
                }
            }
        }
        result
    }

    struct Fixture {
        container: PathBuf,
        manager: IdentityManager,
    }

    impl Fixture {
        fn new() -> Self {
            let container =
                std::env::temp_dir().join(format!("abigail_preview_identity_{}", Uuid::new_v4()));
            std::fs::create_dir_all(&container).unwrap();
            let profile = Arc::new(
                abigail_core::profile::ProfileContext::preflight(&container.join("profile"), &[])
                    .unwrap(),
            );
            let root = profile.root().to_path_buf();
            let manager = IdentityManager {
                global_config: RwLock::new(GlobalConfig::new(&root)),
                data_root: root,
                profile: Some(profile),
                master_key: SigningKey::from_bytes(&[9; 32]),
            };
            Self { container, manager }
        }

        fn reopen(&self) -> IdentityManager {
            let profile = Arc::new(
                abigail_core::profile::ProfileContext::preflight(self.manager.data_root(), &[])
                    .unwrap(),
            );
            IdentityManager {
                data_root: profile.root().to_path_buf(),
                profile: Some(profile),
                global_config: RwLock::new(GlobalConfig::load(self.manager.data_root()).unwrap()),
                master_key: SigningKey::from_bytes(&[9; 32]),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let temporary_root = std::env::temp_dir();
            assert!(self.container.is_absolute());
            assert!(self.container.starts_with(&temporary_root));
            assert_ne!(self.container, temporary_root);
            let _ = std::fs::remove_dir_all(&self.container);
        }
    }

    #[test]
    fn preview_documents_are_registered_uuid_scoped_and_stable_after_rename() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        let (ongoing, _) = manager.create_agent("Same display name").unwrap();
        let (disposable, _) = manager.create_agent("Same display name").unwrap();
        let ongoing_docs = manager.create_documents_folder(&ongoing).unwrap();
        let disposable_docs = manager.create_documents_folder(&disposable).unwrap();
        assert_eq!(
            ongoing_docs,
            manager.data_root().join("Documents").join(&ongoing)
        );
        assert_eq!(
            disposable_docs,
            manager.data_root().join("Documents").join(&disposable)
        );
        assert_ne!(ongoing_docs, disposable_docs);
        std::fs::write(ongoing_docs.join("ongoing.txt"), b"retained").unwrap();
        manager
            .update_agent_name(&ongoing, "Renamed Entity")
            .unwrap();
        assert_eq!(
            manager.create_documents_folder(&ongoing).unwrap(),
            ongoing_docs
        );
        assert!(manager
            .create_documents_folder(&Uuid::new_v4().to_string())
            .is_err());
        assert!(!manager.data_root().join("Documents/Abigail").exists());
        manager.delete_agent(&disposable).unwrap();
        assert_eq!(
            std::fs::read(ongoing_docs.join("ongoing.txt")).unwrap(),
            b"retained"
        );
    }

    #[test]
    fn preview_identity_paths_and_documents_survive_manager_reopen() {
        let fixture = Fixture::new();
        let (id, directory) = fixture.manager.create_agent("Continuing preview").unwrap();
        let before = std::fs::read(directory.join("config.json")).unwrap();
        let documents = fixture.manager.create_documents_folder(&id).unwrap();
        let profile_id = fixture.manager.profile.as_ref().unwrap().profile_id();
        let reopened = fixture.reopen();
        let config = reopened.load_agent(&id).unwrap();
        assert_eq!(reopened.profile.as_ref().unwrap().profile_id(), profile_id);
        assert_eq!(config.data_dir, directory);
        assert_eq!(config.docs_dir, directory.join("docs"));
        assert_eq!(config.models_dir, directory.join("models"));
        assert_eq!(config.db_path, reopened.data_root().join("memory.db"));
        assert_eq!(reopened.agent_dir(&id).unwrap(), directory);
        assert_eq!(reopened.create_documents_folder(&id).unwrap(), documents);
        assert_eq!(
            std::fs::read(directory.join("config.json")).unwrap(),
            before
        );
    }

    #[test]
    fn preview_outside_config_paths_are_rejected_before_migration_or_archive() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        let (id, directory) = manager.create_agent("Continuing preview").unwrap();
        let config_path = directory.join("config.json");
        let original = std::fs::read(&config_path).unwrap();
        let registry_path = GlobalConfig::config_path(manager.data_root());
        let registry = std::fs::read(&registry_path).unwrap();
        let outside = fixture.container.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("sentinel"), b"outside unchanged").unwrap();

        for field in [
            "data_dir",
            "models_dir",
            "docs_dir",
            "db_path",
            "external_pubkey_path",
        ] {
            let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
            value["schema_version"] = serde_json::json!(26);
            value[field] = serde_json::json!(outside.join(field));
            let invalid = serde_json::to_vec_pretty(&value).unwrap();
            std::fs::write(&config_path, &invalid).unwrap();

            assert!(manager.load_agent(&id).is_err(), "{field}");
            assert!(manager.list_agents().is_err(), "{field}");
            assert!(manager.backup_agent(&id).is_err(), "{field}");
            assert!(manager.archive_agent(&id).is_err(), "{field}");
            assert_eq!(std::fs::read(&config_path).unwrap(), invalid, "{field}");
            assert_eq!(std::fs::read(&registry_path).unwrap(), registry, "{field}");
            assert!(!manager.data_root().join("backups").exists());
            assert_eq!(
                std::fs::read(outside.join("sentinel")).unwrap(),
                b"outside unchanged"
            );
        }
    }

    #[test]
    fn preview_rejects_a_different_shared_store_even_inside_the_profile() {
        let fixture = Fixture::new();
        let (id, directory) = fixture.manager.create_agent("Shared store").unwrap();
        let config_path = directory.join("config.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
        value["db_path"] = serde_json::json!(fixture.manager.data_root().join("other-memory.db"));
        let invalid = serde_json::to_vec(&value).unwrap();
        std::fs::write(&config_path, &invalid).unwrap();
        assert!(fixture.manager.load_agent(&id).is_err());
        assert_eq!(std::fs::read(&config_path).unwrap(), invalid);
        assert!(!fixture.manager.data_root().join("other-memory.db").exists());
    }

    #[test]
    fn preview_active_config_cannot_select_another_entity_before_migration() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        let (ongoing, ongoing_dir) = manager.create_agent("Same name").unwrap();
        let (disposable, disposable_dir) = manager.create_agent("Same name").unwrap();
        let ongoing_path = ongoing_dir.join("config.json");
        let disposable_path = disposable_dir.join("config.json");
        let ongoing_bytes = std::fs::read(&ongoing_path).unwrap();
        let registry_path = GlobalConfig::config_path(manager.data_root());
        let registry_bytes = std::fs::read(&registry_path).unwrap();

        // All declared paths are individually valid inside this profile, but
        // belong to the other Entity. An old schema must not migrate first.
        let mut redirected: serde_json::Value = serde_json::from_slice(&ongoing_bytes).unwrap();
        redirected["schema_version"] = serde_json::json!(abigail_core::CONFIG_SCHEMA_VERSION - 1);
        let redirected_bytes = serde_json::to_vec_pretty(&redirected).unwrap();
        std::fs::write(&disposable_path, &redirected_bytes).unwrap();

        assert!(manager.load_agent(&disposable).is_err());
        assert!(manager.list_agents().is_err());
        assert!(manager.backup_agent(&disposable).is_err());
        assert!(manager.archive_agent(&disposable).is_err());
        assert_eq!(std::fs::read(&disposable_path).unwrap(), redirected_bytes);
        assert_eq!(std::fs::read(&ongoing_path).unwrap(), ongoing_bytes);
        assert_eq!(std::fs::read(&registry_path).unwrap(), registry_bytes);
        assert!(!manager.data_root().join("backups").exists());
        assert_eq!(manager.load_agent(&ongoing).unwrap().data_dir, ongoing_dir);
    }

    #[test]
    fn preview_legacy_identity_refusal_preserves_all_source_and_registry_bytes() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        let (_, directory) = manager.create_agent("Continuing Entity").unwrap();
        let root = manager.data_root();
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("config.json")).unwrap()).unwrap();
        legacy["schema_version"] = serde_json::json!(abigail_core::CONFIG_SCHEMA_VERSION - 1);
        legacy["birth_complete"] = serde_json::json!(true);
        legacy["data_dir"] = serde_json::json!(root);
        legacy["docs_dir"] = serde_json::json!(root.join("docs"));
        legacy["models_dir"] = serde_json::json!(root.join("models"));
        std::fs::write(
            root.join("config.json"),
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();
        std::fs::create_dir(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/soul.md"), b"synthetic legacy document").unwrap();
        std::fs::write(
            root.join("secrets.bin"),
            b"synthetic bytes: never decrypted",
        )
        .unwrap();
        std::fs::write(root.join("master.key"), b"synthetic bytes: never loaded").unwrap();
        let before = snapshot(root);

        assert!(manager
            .migrate_legacy_identity()
            .unwrap_err()
            .contains("explicit identity import"));
        assert_eq!(snapshot(root), before);
    }

    #[test]
    fn preview_decryption_failure_never_recovers_or_bootstraps_identity_state() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        let (_, ongoing) = manager.create_agent("Continuing Entity").unwrap();
        manager.create_agent("Disposable Entity").unwrap();
        std::fs::write(ongoing.join("presence.json"), b"maintained presentation").unwrap();
        std::fs::write(
            manager.data_root().join("master.key"),
            b"synthetic incompatible encrypted master key",
        )
        .unwrap();
        let before = snapshot(manager.data_root());
        let loads = std::cell::Cell::new(0);

        let error = IdentityManager::load_with_recovery::<()>(
            manager.profile.is_some(),
            || {
                loads.set(loads.get() + 1);
                Err("AES-GCM decryption failed: wrong key or tampered data".into())
            },
            || {
                panic!("Preview must not invoke identity recovery");
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("automatic reset is disabled"));
        assert_eq!(loads.get(), 1, "No bootstrap/retry after a rejected load");
        assert_eq!(snapshot(manager.data_root()), before);
        assert!(!manager.data_root().join("backups").exists());
    }

    #[test]
    fn preview_missing_master_key_preserves_existing_registry_and_identity_tree() {
        for material in ["registry", "identities", "both"] {
            let fixture = Fixture::new();
            let root = fixture.manager.data_root();
            if material != "identities" {
                GlobalConfig::new(root).save(root).unwrap();
            }
            if material != "registry" {
                let identity = root.join("identities").join(Uuid::new_v4().to_string());
                std::fs::create_dir_all(identity.join("docs")).unwrap();
                std::fs::write(identity.join("presence.json"), b"maintained presentation").unwrap();
                std::fs::write(
                    identity.join("docs/soul.md"),
                    b"maintained identity document",
                )
                .unwrap();
            }
            let before = snapshot(root);

            let error = IdentityManager::validate_preview_bootstrap(root, true).unwrap_err();
            assert!(
                error.contains("master.key is missing"),
                "{material}: {error}"
            );
            assert_eq!(snapshot(root), before, "{material}");
            assert!(!root.join("vault.sentinel").exists());
            assert!(!root.join("master.key").exists());
            // Stable mode keeps its existing missing-key bootstrap behavior.
            assert!(IdentityManager::validate_preview_bootstrap(root, false).is_ok());
            assert_eq!(snapshot(root), before, "stable check: {material}");
        }
    }

    #[test]
    fn preview_bootstrap_allows_fresh_or_existing_keyed_identity_state() {
        let fixture = Fixture::new();
        let root = fixture.manager.data_root();
        let before = snapshot(root);
        assert!(IdentityManager::validate_preview_bootstrap(root, true).is_ok());
        assert_eq!(snapshot(root), before);

        std::fs::create_dir(root.join("identities")).unwrap();
        let empty_identity_state = snapshot(root);
        assert!(IdentityManager::validate_preview_bootstrap(root, true).is_ok());
        assert_eq!(snapshot(root), empty_identity_state);

        GlobalConfig::new(root).save(root).unwrap();
        std::fs::write(
            root.join("master.key"),
            b"synthetic existing key: never unlocked",
        )
        .unwrap();
        let keyed_state = snapshot(root);
        assert!(IdentityManager::validate_preview_bootstrap(root, true).is_ok());
        assert_eq!(snapshot(root), keyed_state);
        assert!(!root.join("vault.sentinel").exists());
    }

    #[test]
    fn stable_identity_decryption_recovery_still_retries_after_recovery() {
        let calls = std::cell::RefCell::new(Vec::new());
        let value = IdentityManager::load_with_recovery(
            false,
            || {
                let retry = calls.borrow().contains(&"recover");
                calls.borrow_mut().push("load");
                if retry {
                    Ok("synthetic recovered identity")
                } else {
                    Err("AES-GCM decryption failed".into())
                }
            },
            || {
                calls.borrow_mut().push("recover");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(value, "synthetic recovered identity");
        assert_eq!(*calls.borrow(), ["load", "recover", "load"]);
    }

    #[test]
    fn preview_restore_rebinds_declared_missing_public_key_for_unborn_entity() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        let (id, directory) = manager.create_agent("Unborn Entity").unwrap();
        let mut config = manager.load_agent(&id).unwrap();
        assert!(!config.birth_complete);
        config.external_pubkey_path = Some(directory.join("external_pubkey.bin"));
        config.save(&directory.join("config.json")).unwrap();
        assert!(!directory.join("external_pubkey.bin").exists());
        let original_bytes = std::fs::read(directory.join("config.json")).unwrap();
        let backup = PathBuf::from(manager.backup_agent(&id).unwrap());

        let restored_id = manager
            .restore_backup(backup.file_name().unwrap().to_str().unwrap())
            .unwrap();
        let restored = manager.load_agent(&restored_id).unwrap();
        assert!(!restored.birth_complete);
        assert_eq!(
            restored.external_pubkey_path,
            Some(restored.data_dir.join("external_pubkey.bin")),
        );
        assert_ne!(restored.data_dir, directory);
        assert!(!restored.external_pubkey_path.unwrap().exists());
        assert_eq!(
            std::fs::read(directory.join("config.json")).unwrap(),
            original_bytes
        );
    }

    #[test]
    fn preview_disposable_backups_restore_inside_root_without_touching_ongoing_files() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        let (ongoing, ongoing_dir) = manager.create_agent("Same name").unwrap();
        let (disposable, disposable_dir) = manager.create_agent("Same name").unwrap();
        std::fs::write(ongoing_dir.join("presence.json"), b"ongoing presentation").unwrap();
        std::fs::write(
            disposable_dir.join("presence.json"),
            b"disposable presentation",
        )
        .unwrap();
        let ongoing_config = std::fs::read(ongoing_dir.join("config.json")).unwrap();
        let first = PathBuf::from(manager.backup_agent(&ongoing).unwrap());
        let second = PathBuf::from(manager.backup_agent(&disposable).unwrap());
        let third = PathBuf::from(manager.backup_agent(&disposable).unwrap());
        assert_ne!(first, second);
        assert_ne!(second, third);
        let backups = manager.list_backups().unwrap();
        assert_eq!(backups.len(), 3);
        assert!(backups
            .iter()
            .all(|backup| Path::new(&backup.directory_path)
                .starts_with(manager.data_root().join("backups"))));

        let restored = manager
            .restore_backup(second.file_name().unwrap().to_str().unwrap())
            .unwrap();
        assert_ne!(restored, disposable);
        let restored_config = manager.load_agent(&restored).unwrap();
        assert_eq!(
            restored_config.data_dir,
            manager.data_root().join("identities").join(&restored)
        );
        assert_eq!(
            restored_config.db_path,
            manager.data_root().join("memory.db")
        );
        assert_eq!(
            std::fs::read(restored_config.data_dir.join("presence.json")).unwrap(),
            b"disposable presentation"
        );
        manager
            .delete_backup(third.file_name().unwrap().to_str().unwrap())
            .unwrap();
        manager.delete_agent(&disposable).unwrap();
        let reopened = fixture.reopen();
        assert_eq!(reopened.load_agent(&ongoing).unwrap().data_dir, ongoing_dir);
        assert_eq!(
            std::fs::read(ongoing_dir.join("config.json")).unwrap(),
            ongoing_config
        );
        assert_eq!(
            std::fs::read(ongoing_dir.join("presence.json")).unwrap(),
            b"ongoing presentation"
        );
    }

    #[test]
    fn preview_outside_backup_config_is_refused_without_consuming_the_backup() {
        let fixture = Fixture::new();
        let manager = &fixture.manager;
        let (id, _) = manager.create_agent("Import fixture").unwrap();
        let backup = PathBuf::from(manager.backup_agent(&id).unwrap());
        let config_path = backup.join("config.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
        value["docs_dir"] = serde_json::json!(fixture.container.join("outside-documents"));
        let invalid = serde_json::to_vec_pretty(&value).unwrap();
        std::fs::write(&config_path, &invalid).unwrap();
        let registry_path = GlobalConfig::config_path(manager.data_root());
        let registry = std::fs::read(&registry_path).unwrap();
        assert!(manager.list_backups().is_err());
        assert!(manager
            .restore_backup(backup.file_name().unwrap().to_str().unwrap())
            .is_err());
        assert_eq!(std::fs::read(&config_path).unwrap(), invalid);
        assert_eq!(std::fs::read(&registry_path).unwrap(), registry);
        assert!(!fixture.container.join("outside-documents").exists());
    }

    #[test]
    fn preview_backup_selection_cannot_target_an_outside_fixture() {
        let fixture = Fixture::new();
        let outside = fixture.container.join("outside-backup");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("sentinel"), b"untouched").unwrap();
        let outside_name = outside.to_str().unwrap();
        assert!(fixture.manager.restore_backup(outside_name).is_err());
        assert!(fixture.manager.delete_backup(outside_name).is_err());
        assert!(fixture.manager.restore_backup(".").is_err());
        assert_eq!(
            std::fs::read(outside.join("sentinel")).unwrap(),
            b"untouched"
        );
        assert!(fixture.manager.list_backups().unwrap().is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manager() -> IdentityManager {
        static VAULT_ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        VAULT_ROOT.get_or_init(|| {
            // Hybrid unlock caches its key per process. Bootstrap one isolated
            // vault before parallel fixtures encrypt birth signing keys, rather
            // than relying on the user's keyring or another test binary's sentinel.
            let root = std::env::temp_dir()
                .join(format!("abigail_identity_test_vault_{}", Uuid::new_v4()));
            std::fs::create_dir_all(&root).expect("create isolated identity test vault");
            abigail_core::vault::unlock::configure_process_vault_data_dir(&root);
            let previous_raw_key = std::env::var_os("ABIGAIL_VAULT_RAW_KEY");
            std::env::set_var("ABIGAIL_VAULT_RAW_KEY", "42".repeat(32));
            use abigail_core::UnlockProvider as _;
            let result = abigail_core::HybridUnlockProvider::new().root_kek();
            match previous_raw_key {
                Some(value) => std::env::set_var("ABIGAIL_VAULT_RAW_KEY", value),
                None => std::env::remove_var("ABIGAIL_VAULT_RAW_KEY"),
            }
            result.expect("bootstrap isolated identity test vault");
            root
        });
        let tmp = std::env::temp_dir().join(format!("abigail_identity_test_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        IdentityManager {
            data_root: tmp.clone(),
            profile: None,
            global_config: RwLock::new(GlobalConfig::new(&tmp)),
            master_key: SigningKey::from_bytes(&[9u8; 32]),
        }
    }

    #[test]
    fn concurrent_unborn_loads_and_creation_complete_without_registry_deadlock() {
        use std::sync::{mpsc, Arc, Barrier};
        use std::time::Duration;

        let manager = Arc::new(test_manager());
        let (entity_id, _) = manager.create_agent("Concurrent unborn reader").unwrap();
        let start = Arc::new(Barrier::new(5));
        let (done_tx, done_rx) = mpsc::channel();
        let mut threads = Vec::new();
        for _ in 0..3 {
            let manager = manager.clone();
            let entity_id = entity_id.clone();
            let start = start.clone();
            let done_tx = done_tx.clone();
            threads.push(std::thread::spawn(move || {
                start.wait();
                let result = (0..200).try_for_each(|_| manager.load_agent(&entity_id).map(|_| ()));
                let _ = done_tx.send(result);
            }));
        }
        {
            let manager = manager.clone();
            let start = start.clone();
            let done_tx = done_tx.clone();
            threads.push(std::thread::spawn(move || {
                start.wait();
                let result = (0..16).try_for_each(|index| {
                    manager
                        .create_agent(&format!("Concurrent creator {}", index))
                        .map(|_| ())
                });
                let _ = done_tx.send(result);
            }));
        }
        drop(done_tx);
        start.wait();
        for _ in 0..4 {
            done_rx
                .recv_timeout(Duration::from_secs(20))
                .expect(
                    "concurrent create/load must finish without a recursive registry-lock deadlock",
                )
                .expect("identity operation succeeds");
        }
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(manager.list_agents().unwrap().len(), 17);
        let _ = std::fs::remove_dir_all(manager.data_root());
    }

    #[test]
    fn custom_data_root_keeps_documents_inside_that_root() {
        let manager = test_manager();
        let (id, _) = manager.create_agent("Isolated Documents").unwrap();
        let documents = manager.create_documents_folder(&id).unwrap();
        let expected_base = std::env::var_os("ABIGAIL_DOCUMENTS_DIR")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| manager.data_root().join("Documents"));
        assert_eq!(
            documents,
            expected_base.join("Abigail").join("Isolated Documents")
        );
        let _ = std::fs::remove_dir_all(manager.data_root());
    }

    #[test]
    fn birth_signing_survives_reload_and_detects_document_tampering() {
        let manager = test_manager();
        let (id, dir) = manager.create_agent("Birth trust test").unwrap();
        for name in ["soul.md", "ethics.md", "instincts.md"] {
            std::fs::write(dir.join("docs").join(name), format!("Original {}", name)).unwrap();
        }
        let public_path = manager.sign_birth_documents(&id).unwrap();
        let original_public = std::fs::read(&public_path).unwrap();
        let mut config = manager.load_agent(&id).unwrap();
        config.external_pubkey_path = Some(public_path.clone());
        config.birth_complete = true;
        config.save(&config.config_path()).unwrap();
        manager.verify_agent(&id).unwrap();
        abigail_core::verifier::verify_constitutional_integrity(&config).unwrap();

        let reopened = IdentityManager {
            data_root: manager.data_root.clone(),
            profile: manager.profile.clone(),
            global_config: RwLock::new(GlobalConfig::load(manager.data_root()).unwrap()),
            master_key: manager.master_key.clone(),
        };
        let loaded = reopened.load_agent(&id).unwrap();
        abigail_core::verifier::verify_constitutional_integrity(&loaded).unwrap();
        std::fs::write(dir.join("docs/soul.md"), "Mentor-approved new purpose").unwrap();
        assert!(abigail_core::verifier::verify_constitutional_integrity(&loaded).is_err());
        reopened.sign_birth_documents(&id).unwrap();
        assert_eq!(std::fs::read(public_path).unwrap(), original_public);
        abigail_core::verifier::verify_constitutional_integrity(&loaded).unwrap();
        let encrypted = std::fs::read(dir.join("birth_signing_key.vault")).unwrap();
        assert!(abigail_core::vault::crypto::is_vault_envelope(&encrypted));
        let _ = std::fs::remove_dir_all(manager.data_root());
    }

    #[test]
    fn artifact_verification_uses_the_hive_key_and_rejects_tampering() {
        let manager = test_manager();
        let (signature, key) = manager.sign_payload(b"birth certificate");
        manager
            .verify_payload(b"birth certificate", &signature, &key)
            .unwrap();
        assert!(manager
            .verify_payload(b"changed certificate", &signature, &key)
            .is_err());
        assert!(manager
            .verify_payload(b"birth certificate", &signature, &"00".repeat(32))
            .is_err());
        assert!(manager
            .verify_payload(b"birth certificate", "not hex", &key)
            .is_err());
        let _ = std::fs::remove_dir_all(manager.data_root());
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
    fn aes_gcm_errors_trigger_identity_auto_reset() {
        assert!(IdentityManager::should_auto_reset_identity_state(
            "AES-GCM decryption failed (wrong key or tampered data)"
        ));
        assert!(!IdentityManager::should_auto_reset_identity_state(
            "permission denied while reading config"
        ));
    }

    #[test]
    fn recover_identity_state_archives_only_identity_material() {
        let tmp =
            std::env::temp_dir().join(format!("abigail_identity_recovery_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("identities").join("adam").join("docs")).unwrap();
        std::fs::create_dir_all(tmp.join("docs")).unwrap();

        std::fs::write(tmp.join("master.key"), b"not-a-real-vault").unwrap();
        std::fs::write(
            GlobalConfig::config_path(&tmp),
            br#"{"master_key_path":"master.key","agents":[],"default_theme":"modern"}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("identities")
                .join("adam")
                .join("docs")
                .join("journal.md"),
            b"identity-doc",
        )
        .unwrap();
        std::fs::write(tmp.join("memory.db"), b"hive-memory").unwrap();
        std::fs::write(tmp.join("docs").join("family.md"), b"shared-doc").unwrap();

        IdentityManager::recover_identity_state(&tmp).unwrap();

        assert!(!tmp.join("master.key").exists());
        assert!(!GlobalConfig::config_path(&tmp).exists());
        assert!(!tmp.join("identities").exists());
        assert!(tmp.join("memory.db").exists());
        assert!(tmp.join("docs").join("family.md").exists());

        let recovery_dir = std::fs::read_dir(tmp.join("backups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(recovery_dir.join("master.key").exists());
        assert!(recovery_dir.join("global_settings.json").exists());
        assert!(recovery_dir
            .join("identities")
            .join("adam")
            .join("docs")
            .join("journal.md")
            .exists());

        let _ = std::fs::remove_dir_all(&tmp);
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
            profile: None,
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
        assert_eq!(migrated_config.db_path, manager.shared_memory_db_path());
        assert_eq!(
            migrated_config.external_pubkey_path,
            Some(agent_dir.join("external_pubkey.bin"))
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn bootstrap_creates_immortal_hive_entity() {
        let tmp = std::env::temp_dir().join(format!("abigail_identity_hive_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let mut global = GlobalConfig::new(&tmp);
        IdentityManager::ensure_hive_entity(&tmp, &mut global).unwrap();
        let manager = IdentityManager {
            data_root: tmp.clone(),
            profile: None,
            global_config: RwLock::new(global.clone()),
            master_key: SigningKey::from_bytes(&[9u8; 32]),
        };

        let hive = global
            .agents
            .iter()
            .find(|entry| entry.is_hive)
            .cloned()
            .expect("hive entry");
        let config = manager.load_agent(&hive.id).unwrap();

        assert!(config.is_hive);
        assert_eq!(config.agent_name.as_deref(), Some(HIVE_ENTITY_NAME));
        assert_eq!(config.db_path, HiveEntity::memory_db_path(&tmp));

        let listed = manager.list_agents().unwrap();
        assert!(listed.iter().any(|agent| agent.is_hive && agent.immortal));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn hive_entity_cannot_be_deleted_or_archived() {
        let tmp =
            std::env::temp_dir().join(format!("abigail_identity_hive_guard_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let mut global = GlobalConfig::new(&tmp);
        IdentityManager::ensure_hive_entity(&tmp, &mut global).unwrap();

        let manager = IdentityManager {
            data_root: tmp.clone(),
            profile: None,
            global_config: RwLock::new(global),
            master_key: SigningKey::from_bytes(&[9u8; 32]),
        };

        let hive_id = manager.hive_agent_id().unwrap();
        assert!(manager.delete_agent(&hive_id).is_err());
        assert!(manager.archive_agent(&hive_id).is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
