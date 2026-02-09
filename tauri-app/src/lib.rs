#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod identity_manager;
mod templates;

use abigail_birth::BirthOrchestrator;
use abigail_core::{
    generate_external_keypair, sign_constitutional_documents, validate_local_llm_url, AppConfig,
    CoreError, ExternalVault, Keyring, McpServerDefinition, ReadOnlyFileVault, SecretsVault,
    TrinityConfig, Verifier,
};
use abigail_memory::{Memory, MemoryStore};
use abigail_router::{IdEgoRouter, SubagentManager};
use abigail_skills::channel::EventBus;
use abigail_skills::protocol::mcp::{HttpMcpClient, McpTool};
use abigail_skills::{MissingSkillSecret, SkillExecutor, SkillRegistry, ToolParams};
use base64::Engine as _;
use chrono::Utc;
use ed25519_dalek::SigningKey;
use identity_manager::{AgentIdentityInfo, IdentityManager};
use regex::Regex;
use serde::{Deserialize, Serialize};
use skill_filesystem::FilesystemSkill;
use skill_http::HttpSkill;
use skill_perplexity_search::PerplexitySearchSkill;
use skill_shell::ShellSkill;
use skill_web_search::WebSearchSkill;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use tauri::Emitter;

/// Returns permitted base directories for backup destinations (canonicalized where possible).
/// Backup is allowed only under: data_dir, or user's Documents/Home.
fn permitted_backup_bases(data_dir: &Path) -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Ok(canon) = data_dir.canonicalize() {
        bases.push(canon);
    } else {
        bases.push(data_dir.to_path_buf());
    }
    #[cfg(windows)]
    if let Ok(profile) = std::env::var("USERPROFILE") {
        bases.push(PathBuf::from(&profile).join("Documents"));
    }
    #[cfg(not(windows))]
    if let Ok(home) = std::env::var("HOME") {
        bases.push(PathBuf::from(&home).join("Documents"));
        bases.push(PathBuf::from(&home));
    }
    bases
}

/// Recursively copy a directory (for skill package install).
fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), &dst_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Append a line to the skill audit log (data_dir/skill_audit.log).
fn skill_audit_log(data_dir: &Path, action: &str, detail: &str) {
    let log_path = data_dir.join("skill_audit.log");
    let line = format!(
        "{} {} {}\n",
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        action,
        detail
    );
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

/// Validates that dest_path is under a permitted base (data_dir or user Documents/Home).
/// Prevents path traversal and arbitrary file write.
fn validate_backup_dest_path(dest_path: &str, data_dir: &Path) -> Result<PathBuf, String> {
    let path = Path::new(dest_path);
    if path.has_root() && path.components().count() == 0 {
        return Err("Invalid backup path".into());
    }
    let parent = path.parent().ok_or("Invalid path: no parent directory")?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| format!("Path parent is not accessible or does not exist: {}", e))?;
    let permitted = permitted_backup_bases(data_dir);
    for base in &permitted {
        let canonical_base = match base.canonicalize() {
            Ok(b) => b,
            Err(_) => base.clone(),
        };
        if canonical_parent.starts_with(&canonical_base) {
            return Ok(path.to_path_buf());
        }
    }
    Err("Backup path must be under the app data directory or your Documents/Home folder".into())
}

/// Redact API keys from text to prevent leaking them in transcripts.
/// Matches common key prefixes: sk-..., sk-ant-..., sk-proj-..., xai-..., pplx-..., AIza...
fn redact_api_keys(text: &str) -> String {
    // Lazy-init regex for common API key patterns
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"(?x)
            (?:sk-(?:ant-|proj-)?[A-Za-z0-9_-]{10,}) |   # OpenAI / Anthropic
            (?:xai-[A-Za-z0-9_-]{10,})               |   # xAI
            (?:pplx-[A-Za-z0-9_-]{10,})              |   # Perplexity
            (?:AIza[A-Za-z0-9_-]{10,})                    # Google
            ",
        )
        .expect("redact regex")
    });
    re.replace_all(text, |caps: &regex::Captures| {
        let m = caps.get(0).unwrap().as_str();
        // Keep the prefix visible (up to first dash + 3 chars), mask the rest
        let visible = if let Some(pos) = m.find('-') {
            let end = (pos + 4).min(m.len());
            &m[..end]
        } else {
            &m[..4.min(m.len())]
        };
        format!("{}***", visible)
    })
    .into_owned()
}

struct AppState {
    config: RwLock<AppConfig>,
    birth: RwLock<Option<BirthOrchestrator>>,
    router: RwLock<IdEgoRouter>,
    registry: Arc<SkillRegistry>,
    executor: Arc<SkillExecutor>,
    #[allow(dead_code)] // used for skill-event subscription; keep for future UI wiring
    event_bus: Arc<EventBus>,
    secrets: Arc<Mutex<SecretsVault>>,
    /// Hive-level secrets vault (shared API keys across all agents)
    hive_secrets: Arc<Mutex<SecretsVault>>,
    /// Identity manager for the Hive multi-agent system
    identity_manager: Arc<IdentityManager>,
    /// Currently active agent UUID (None if no agent loaded)
    active_agent_id: RwLock<Option<String>>,
    /// Subagent manager for delegating tasks to specialized subagents
    subagent_manager: RwLock<SubagentManager>,
}

fn get_config() -> AppConfig {
    let mut config = AppConfig::default_paths();
    let path = config.config_path();
    if path.exists() {
        config = AppConfig::load(&path).unwrap_or(config);
    }

    // SSRF: ensure loaded local LLM URL is still valid (e.g. config may have been tampered)
    if let Some(ref url) = config.local_llm_base_url {
        if let Ok(normalized) = validate_local_llm_url(url) {
            config.local_llm_base_url = Some(normalized);
        } else {
            tracing::warn!("Config local_llm_base_url rejected (SSRF validation), clearing");
            config.local_llm_base_url = None;
        }
    }

    // Environment variable fallbacks
    if config.local_llm_base_url.is_none() {
        if let Ok(env_url) = std::env::var("LOCAL_LLM_BASE_URL") {
            if !env_url.is_empty() {
                if let Ok(normalized) = validate_local_llm_url(&env_url) {
                    config.local_llm_base_url = Some(normalized);
                } else {
                    tracing::warn!("LOCAL_LLM_BASE_URL from env rejected (SSRF validation)");
                }
            }
        }
    }
    if config.openai_api_key.is_none() {
        config.openai_api_key = std::env::var("OPENAI_API_KEY")
            .ok()
            .filter(|s| !s.is_empty());
    }

    config
}

// ── Hive Identity Management ─────────────────────────────────────────

/// Check if the Hive has been initialized (master key + global settings exist).
#[tauri::command]
fn check_hive_status(state: tauri::State<AppState>) -> Result<bool, String> {
    Ok(state.identity_manager.has_agents()
        || state
            .identity_manager
            .data_root()
            .join("master.key")
            .exists())
}

/// Get the list of all registered agent identities.
#[tauri::command]
fn get_identities(state: tauri::State<AppState>) -> Result<Vec<AgentIdentityInfo>, String> {
    state.identity_manager.list_agents()
}

/// Get the currently active agent UUID.
#[tauri::command]
fn get_active_agent(state: tauri::State<AppState>) -> Result<Option<String>, String> {
    let active = state.active_agent_id.read().map_err(|e| e.to_string())?;
    Ok(active.clone())
}

/// Load an agent by UUID. Verifies signature, loads config into AppState.
#[tauri::command]
fn load_agent(state: tauri::State<AppState>, agent_id: String) -> Result<(), String> {
    // Verify and load agent config
    let agent_config = state.identity_manager.load_agent(&agent_id)?;

    // Update AppState with the loaded agent's config
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        *config = agent_config;
    }

    // Update secrets vault to point to agent's data directory
    {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let mut vault = state.secrets.lock().map_err(|e| e.to_string())?;
        *vault = SecretsVault::load(config.data_dir.clone())
            .unwrap_or_else(|_| SecretsVault::new(config.data_dir.clone()));
    }

    // Rebuild router with new agent's config
    rebuild_router_with_superego(&state)?;

    // Set active agent
    {
        let mut active = state.active_agent_id.write().map_err(|e| e.to_string())?;
        *active = Some(agent_id);
    }

    Ok(())
}

/// Create a new agent identity. Returns the UUID of the created agent.
#[tauri::command]
fn create_agent(state: tauri::State<AppState>, name: String) -> Result<String, String> {
    let (uuid, _agent_dir) = state.identity_manager.create_agent(&name)?;
    Ok(uuid)
}

/// Disconnect from the current agent (return to management screen).
#[tauri::command]
fn disconnect_agent(state: tauri::State<AppState>) -> Result<(), String> {
    let mut active = state.active_agent_id.write().map_err(|e| e.to_string())?;
    *active = None;

    // Reset birth orchestrator
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    *birth = None;

    Ok(())
}

/// Attempt to migrate a legacy single-identity installation to the Hive format.
/// Returns the migrated agent UUID if successful, null if no legacy identity found.
#[tauri::command]
fn migrate_legacy_identity(state: tauri::State<AppState>) -> Result<Option<String>, String> {
    state.identity_manager.migrate_legacy_identity()
}

#[tauri::command]
fn get_birth_complete(state: tauri::State<AppState>) -> Result<bool, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.birth_complete)
}

#[tauri::command]
fn get_agent_name(state: tauri::State<AppState>) -> Result<Option<String>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.agent_name.clone())
}

#[tauri::command]
fn get_docs_path(state: tauri::State<AppState>) -> Result<PathBuf, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.docs_dir.clone())
}

/// One-time setup: copy constitutional docs (without signatures).
/// Signatures are generated separately by generate_and_sign_constitutional.
/// Idempotent if docs already exist.
#[tauri::command]
fn init_soul(state: tauri::State<AppState>) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let data_dir = config.data_dir.clone();
    let docs_dir = config.docs_dir.clone();

    std::fs::create_dir_all(&docs_dir).map_err(|e| e.to_string())?;

    // Copy constitutional docs (without signatures - those come from generate_and_sign_constitutional)
    let docs = [
        ("soul.md", templates::SOUL_MD),
        ("ethics.md", templates::ETHICS_MD),
        ("instincts.md", templates::INSTINCTS_MD),
    ];

    for (name, content) in docs {
        let doc_path = docs_dir.join(name);

        // Only write if not already present (idempotent)
        if !doc_path.exists() {
            std::fs::write(&doc_path, content).map_err(|e| e.to_string())?;
        }
    }

    // Generate internal keyring if not present (for mentor key, etc.)
    let keys_file = data_dir.join("keys.bin");
    if !keys_file.exists() {
        let keyring = Keyring::generate(data_dir).map_err(|e| e.to_string())?;
        keyring.save().map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Generate the external signing keypair and sign constitutional documents.
///
/// This is called during first-run setup. It:
/// 1. Generates a new Ed25519 keypair (or detects if pubkey already exists)
/// 2. Signs the constitutional documents (soul.md, ethics.md, instincts.md)
/// 3. Stores the PUBLIC key in data_dir/external_pubkey.bin
/// 4. Returns the PRIVATE key as base64 for the user to save
///
/// CRITICAL: The private key is returned ONCE and never stored by Abigail.
/// The user MUST save it securely. Without it, they cannot verify integrity
/// after a reinstall or re-sign documents if needed.
#[tauri::command]
fn generate_and_sign_constitutional(
    state: tauri::State<AppState>,
) -> Result<KeypairGenerationResult, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let data_dir = config.data_dir.clone();
    let docs_dir = config.docs_dir.clone();
    drop(config); // Release the lock before doing file I/O

    let pubkey_path = data_dir.join("external_pubkey.bin");

    // Check if we already have signatures (idempotent - don't regenerate)
    let sig_exists = docs_dir.join("soul.md.sig").exists()
        && docs_dir.join("ethics.md.sig").exists()
        && docs_dir.join("instincts.md.sig").exists()
        && pubkey_path.exists();

    if sig_exists {
        // Already generated - can't return the private key again (it was never stored)
        return Err("Constitutional documents are already signed. \
             The private key was presented during initial setup and is not stored by Abigail. \
             If you need to re-sign, you must use your saved private key."
            .to_string());
    }

    // Generate the external keypair
    let keypair_result = generate_external_keypair(&data_dir).map_err(|e| e.to_string())?;

    // Parse the private key from the result to use for signing
    let private_key_bytes = base64::engine::general_purpose::STANDARD
        .decode(&keypair_result.private_key_base64)
        .map_err(|e| format!("Failed to decode private key: {}", e))?;

    let key_bytes: [u8; 32] = private_key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "Invalid private key length")?;

    let signing_key = SigningKey::from_bytes(&key_bytes);

    // Sign the constitutional documents
    sign_constitutional_documents(&signing_key, &docs_dir).map_err(|e| e.to_string())?;

    // Update config to point to the new pubkey
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.external_pubkey_path = Some(pubkey_path.clone());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    Ok(KeypairGenerationResult {
        private_key_base64: keypair_result.private_key_base64,
        public_key_path: pubkey_path.to_string_lossy().to_string(),
        newly_generated: true,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum IdentityStatus {
    Clean,    // No pubkey, no sigs (First Run)
    Complete, // Pubkey exists, all sigs exist
    Broken,   // Pubkey exists, but sigs missing
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterruptedBirthInfo {
    pub was_interrupted: bool,
    pub stage: Option<String>,
}

/// Check if birth was interrupted (closed mid-way through the process).
/// If interrupted, the birth_stage is reset and user must restart from Darkness.
#[tauri::command]
fn check_interrupted_birth(state: tauri::State<AppState>) -> Result<InterruptedBirthInfo, String> {
    let mut config = state.config.write().map_err(|e| e.to_string())?;

    let stage_before = config.birth_stage.clone();
    let was_interrupted = config.check_interrupted_birth();

    if was_interrupted {
        // Save the cleared state
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    Ok(InterruptedBirthInfo {
        was_interrupted,
        stage: stage_before,
    })
}

/// Check the identity status of the application.
#[tauri::command]
fn check_identity_status(state: tauri::State<AppState>) -> Result<IdentityStatus, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let data_dir = config.data_dir.clone();
    let docs_dir = config.docs_dir.clone();

    let pubkey_path = data_dir.join("external_pubkey.bin");
    let pubkey_exists = pubkey_path.exists();

    let sigs_exist = docs_dir.join("soul.md.sig").exists()
        && docs_dir.join("ethics.md.sig").exists()
        && docs_dir.join("instincts.md.sig").exists();

    if !pubkey_exists {
        return Ok(IdentityStatus::Clean);
    }

    if sigs_exist {
        Ok(IdentityStatus::Complete)
    } else {
        Ok(IdentityStatus::Broken)
    }
}

// ── Identity Collision Protocol ─────────────────────────────────────────

/// Summary of an existing identity for the conflict screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentitySummary {
    pub name: String,
    pub birth_date: String,
    pub data_path: String,
    pub has_memories: bool,
    pub has_signatures: bool,
}

/// Check for existing completed identity. Returns summary if found.
/// Used at startup to detect if user should be shown the identity conflict screen.
/// Checks both the birth_complete config flag AND signed identity files on disk
/// to catch stale/interrupted births and version upgrades.
#[tauri::command]
fn check_existing_identity(
    state: tauri::State<AppState>,
) -> Result<Option<IdentitySummary>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;

    // Check for signed identity files on disk (catches upgrades and stale state)
    let has_signed_identity = config.data_dir.join("external_pubkey.bin").exists()
        && config.docs_dir.join("soul.md.sig").exists()
        && config.docs_dir.join("ethics.md.sig").exists()
        && config.docs_dir.join("instincts.md.sig").exists();

    // Return None only if birth is not complete AND no signed identity exists on disk
    if !config.birth_complete && !has_signed_identity {
        return Ok(None);
    }

    let name = config
        .agent_name
        .clone()
        .unwrap_or_else(|| "Unknown".to_string());

    // Get birth date from config or fall back to soul.md file modification time
    let birth_date = config.birth_timestamp.clone().unwrap_or_else(|| {
        let soul_path = config.docs_dir.join("soul.md");
        if let Ok(meta) = std::fs::metadata(&soul_path) {
            if let Ok(mtime) = meta.modified() {
                return chrono::DateTime::<chrono::Utc>::from(mtime)
                    .format("%Y-%m-%d")
                    .to_string();
            }
        }
        "Unknown".to_string()
    });

    let db_path = config.db_path.clone();
    let has_memories = db_path.exists();
    let has_signatures = config.docs_dir.join("soul.md.sig").exists()
        && config.docs_dir.join("ethics.md.sig").exists()
        && config.docs_dir.join("instincts.md.sig").exists();

    Ok(Some(IdentitySummary {
        name,
        birth_date,
        data_path: config.data_dir.to_string_lossy().to_string(),
        has_memories,
        has_signatures,
    }))
}

/// Archive the current identity to a backup folder.
/// Moves all identity files to backups/{timestamp}_{AgentName}/.
/// Returns the backup path on success.
#[tauri::command]
fn archive_identity(state: tauri::State<AppState>) -> Result<String, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let data_dir = config.data_dir.clone();
    let agent_name = config
        .agent_name
        .clone()
        .unwrap_or_else(|| "agent".to_string());
    drop(config);

    // Create backup folder: backups/{timestamp}_{AgentName}/
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let safe_name = agent_name.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_");
    let backup_name = format!("{}_{}", timestamp, safe_name);
    let backups_dir = data_dir.join("backups");
    let backup_path = backups_dir.join(&backup_name);

    std::fs::create_dir_all(&backup_path)
        .map_err(|e| format!("Failed to create backup dir: {}", e))?;

    // Files to archive
    let files_to_move = [
        "config.json",
        "abigail_seed.db",
        "abigail_seed.db-wal",
        "abigail_seed.db-shm",
        "secrets.bin",
        "keys.bin",
        "external_pubkey.bin",
    ];

    for file in &files_to_move {
        let src = data_dir.join(file);
        if src.exists() {
            let dst = backup_path.join(file);
            std::fs::rename(&src, &dst).map_err(|e| format!("Failed to move {}: {}", file, e))?;
        }
    }

    // Move docs/ folder
    let docs_src = data_dir.join("docs");
    if docs_src.exists() {
        let docs_dst = backup_path.join("docs");
        std::fs::rename(&docs_src, &docs_dst).map_err(|e| format!("Failed to move docs: {}", e))?;
    }

    // Reset config in memory to fresh state
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        *config = AppConfig::default_paths();
    }

    // Reset secrets vault
    {
        let mut vault = state.secrets.lock().map_err(|e| e.to_string())?;
        *vault = abigail_core::SecretsVault::new(data_dir.clone());
    }

    tracing::info!("Identity archived");
    Ok(backup_path.to_string_lossy().to_string())
}

/// Completely wipe the current identity. This is irreversible.
/// Deletes all identity files from the data directory.
#[tauri::command]
fn wipe_identity(state: tauri::State<AppState>) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let data_dir = config.data_dir.clone();
    drop(config);

    // Files to delete
    let files_to_delete = [
        "config.json",
        "abigail_seed.db",
        "abigail_seed.db-wal",
        "abigail_seed.db-shm",
        "secrets.bin",
        "keys.bin",
        "external_pubkey.bin",
    ];

    for file in &files_to_delete {
        let path = data_dir.join(file);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("Failed to delete {}: {}", file, e))?;
        }
    }

    // Delete docs/ folder
    let docs_path = data_dir.join("docs");
    if docs_path.exists() {
        std::fs::remove_dir_all(&docs_path).map_err(|e| format!("Failed to delete docs: {}", e))?;
    }

    // Reset config in memory to fresh state
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        *config = AppConfig::default_paths();
    }

    // Reset secrets vault
    {
        let mut vault = state.secrets.lock().map_err(|e| e.to_string())?;
        *vault = abigail_core::SecretsVault::new(data_dir);
    }

    tracing::warn!("Identity wiped - all data deleted");
    Ok(())
}

// ── SQLite Management (The Archives) ────────────────────────────────────

/// Statistics about the SQLite memory database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteStats {
    pub size_bytes: u64,
    pub memory_count: u64,
    pub has_birth: bool,
}

/// Get statistics about the SQLite memory database.
#[tauri::command]
fn get_sqlite_stats(state: tauri::State<AppState>) -> Result<SqliteStats, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let db_path = config.db_path.clone();

    let size_bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

    let store = MemoryStore::open_with_config(&*config).map_err(|e| e.to_string())?;
    let memory_count = store.count_memories().map_err(|e| e.to_string())?;
    let has_birth = store.has_birth().map_err(|e| e.to_string())?;

    Ok(SqliteStats {
        size_bytes,
        memory_count,
        has_birth,
    })
}

/// Optimize the SQLite database by running VACUUM.
/// Returns the number of bytes saved (before - after).
#[tauri::command]
fn optimize_sqlite(state: tauri::State<AppState>) -> Result<i64, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let db_path = config.db_path.clone();

    let size_before = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

    let store = MemoryStore::open_with_config(&*config).map_err(|e| e.to_string())?;
    store.vacuum().map_err(|e| e.to_string())?;

    // Need to drop the store to release the lock before checking size
    drop(store);

    let size_after = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

    let saved = size_before as i64 - size_after as i64;
    tracing::info!("SQLite optimized: {} bytes saved", saved);
    Ok(saved)
}

/// Backup the SQLite database to the specified path.
/// Path must be under data_dir or user Documents/Home (validated to prevent path traversal).
#[tauri::command]
fn backup_sqlite(state: tauri::State<AppState>, dest_path: String) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let db_path = config.db_path.clone();
    let data_dir = config.data_dir.clone();

    let dest_path = validate_backup_dest_path(dest_path.trim(), &data_dir)?;

    // Also copy WAL and SHM files if they exist
    std::fs::copy(&db_path, &dest_path).map_err(|e| format!("Failed to copy database: {}", e))?;

    // Copy WAL file if it exists
    let wal_path = db_path.with_extension("db-wal");
    let dest_wal = dest_path.with_extension("db-wal");
    if wal_path.exists() {
        let _ = std::fs::copy(&wal_path, &dest_wal);
    }

    tracing::info!("SQLite backup completed");
    Ok(())
}

/// Reset all memories (but keep birth record).
/// Returns the number of memories deleted.
#[tauri::command]
fn reset_memories(state: tauri::State<AppState>) -> Result<u64, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let store = MemoryStore::open_with_config(&*config).map_err(|e| e.to_string())?;
    let deleted = store.clear_memories().map_err(|e| e.to_string())?;
    tracing::warn!("Reset memories: {} memories deleted", deleted);
    Ok(deleted)
}

/// Result of startup checks (heartbeat + signature verification).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupCheckResult {
    pub heartbeat_ok: bool,
    pub verification_ok: bool,
    pub error: Option<String>,
}

/// Result of generating and signing with the external keypair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeypairGenerationResult {
    /// Base64-encoded private key. User MUST save this securely.
    pub private_key_base64: String,
    /// Path where the public key was saved.
    pub public_key_path: String,
    /// Whether this is a fresh generation (true) or existing key was found (false).
    pub newly_generated: bool,
}

/// Run startup checks: LLM heartbeat then signature verification.
/// Returns status for each check so the UI can show appropriate messages.
/// When birth is not complete, softens heartbeat requirement.
#[tauri::command]
async fn run_startup_checks(
    state: tauri::State<'_, AppState>,
) -> Result<StartupCheckResult, String> {
    // Check if birth is complete
    let birth_complete = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        config.birth_complete
    };

    // 1. LLM heartbeat — clone router before async boundary (RwLock is not Send)
    let router = state.router.read().map_err(|e| e.to_string())?.clone();
    let heartbeat_result = router.heartbeat().await;
    let heartbeat_ok = heartbeat_result.is_ok();
    let heartbeat_error = heartbeat_result.err().map(|e| e.to_string());

    // During birth, heartbeat failure is non-fatal (birth handles LLM setup)
    if !heartbeat_ok && birth_complete {
        return Ok(StartupCheckResult {
            heartbeat_ok: false,
            verification_ok: false,
            error: heartbeat_error,
        });
    }

    // 2. Signature verification
    let verification_result = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let docs_dir = config.docs_dir.clone();
        let external_pubkey_path = config.effective_external_pubkey_path();

        // Use external vault if configured/auto-detected, otherwise skip (dev mode)
        match external_pubkey_path {
            Some(path) => {
                let vault = ReadOnlyFileVault::new(&path);
                match Verifier::from_vault(&vault) {
                    Ok(mut verifier) => verifier.verify_soul(&docs_dir),
                    Err(e) => Err(e),
                }
            }
            None => {
                // For MVP/dev: if no external pubkey configured, skip verification with warning
                tracing::warn!(
                    "No external_pubkey_path configured; signature verification skipped (dev mode)"
                );
                Ok(())
            }
        }
    };

    let verification_ok = verification_result.is_ok();
    let verification_error = verification_result.err().map(|e| e.to_string());

    Ok(StartupCheckResult {
        heartbeat_ok: heartbeat_ok || !birth_complete,
        verification_ok,
        error: verification_error.or(if !heartbeat_ok { heartbeat_error } else { None }),
    })
}

#[tauri::command]
fn get_birth_stage(state: tauri::State<AppState>) -> Result<String, String> {
    let birth = state.birth.read().map_err(|e| e.to_string())?;
    Ok(birth
        .as_ref()
        .map(|b| b.current_stage().name().to_string())
        .unwrap_or_else(|| "None".to_string()))
}

#[tauri::command]
fn get_birth_message(state: tauri::State<AppState>) -> Result<String, String> {
    let birth = state.birth.read().map_err(|e| e.to_string())?;
    Ok(birth
        .as_ref()
        .map(|b| b.display_message().to_string())
        .unwrap_or_else(|| "".to_string()))
}

#[tauri::command]
fn start_birth(state: tauri::State<AppState>) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let config = config.clone();
    let orchestrator = BirthOrchestrator::new(config).map_err(|e| e.to_string())?;
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    *birth = Some(orchestrator);
    Ok(())
}

#[tauri::command]
fn verify_crypto(state: tauri::State<AppState>) -> Result<(), String> {
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;

    // Use the config's docs_dir as the source of truth
    let docs_path = b.config().docs_dir.clone();

    tracing::info!("Verifying crypto integrity");

    // In the new flow, generate_identity replaces verify_crypto for first run
    // Keep this for legacy/repair path
    b.generate_identity(&docs_path).map_err(|e| {
        tracing::error!("Identity generation failed: {}", e);
        e.to_string()
    })
}

/// Generate identity during Darkness stage: keypair generation, hold signing key.
/// Returns the private key base64 and public key path.
#[tauri::command]
fn generate_identity(state: tauri::State<AppState>) -> Result<KeypairGenerationResult, String> {
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;

    let docs_path = b.config().docs_dir.clone();
    b.generate_identity(&docs_path).map_err(|e| e.to_string())?;

    let private_key = b
        .get_private_key_base64()
        .ok_or("No private key generated")?
        .to_string();

    let data_dir = b.config().data_dir.clone();
    let pubkey_path = data_dir.join("external_pubkey.bin");

    // Also sync config to AppState
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.external_pubkey_path = Some(pubkey_path.clone());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    Ok(KeypairGenerationResult {
        private_key_base64: private_key,
        public_key_path: pubkey_path.to_string_lossy().to_string(),
        newly_generated: true,
    })
}

/// Advance past Darkness after user has saved their private key.
#[tauri::command]
fn advance_past_darkness(state: tauri::State<AppState>) -> Result<(), String> {
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;
    b.advance_past_darkness().map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairIdentityParams {
    pub private_key: Option<String>,
    pub reset: bool,
}

#[tauri::command]
fn repair_identity(
    state: tauri::State<AppState>,
    params: RepairIdentityParams,
) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let data_dir = config.data_dir.clone();
    let docs_dir = config.docs_dir.clone();
    drop(config); // Release lock

    if params.reset {
        tracing::warn!("Performing identity HARD RESET");
        // Delete external_pubkey.bin
        let pubkey_path = data_dir.join("external_pubkey.bin");
        if pubkey_path.exists() {
            std::fs::remove_file(&pubkey_path).map_err(|e| e.to_string())?;
        }
        // Delete all .sig files
        for doc in ["soul.md", "ethics.md", "instincts.md"] {
            let sig_path = docs_dir.join(format!("{}.sig", doc));
            if sig_path.exists() {
                std::fs::remove_file(&sig_path).map_err(|e| e.to_string())?;
            }
        }
        return Ok(());
    }

    if let Some(private_key_base64) = params.private_key {
        tracing::info!("Attempting identity REPAIR with provided private key");

        // 1. Validate private key format
        let private_key_bytes = base64::engine::general_purpose::STANDARD
            .decode(&private_key_base64)
            .map_err(|e| format!("Invalid private key format: {}", e))?;

        let key_bytes: [u8; 32] = private_key_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "Invalid private key length")?;

        let signing_key = SigningKey::from_bytes(&key_bytes);
        let verifying_key = signing_key.verifying_key();

        // 2. Validate against stored public key
        let pubkey_path = data_dir.join("external_pubkey.bin");
        if !pubkey_path.exists() {
            return Err(
                "Public key not found. Cannot verify ownership. Please use Reset.".to_string(),
            );
        }

        let vault = ReadOnlyFileVault::new(&pubkey_path);
        let stored_pubkey = vault
            .read_public_key()
            .map_err(|e: CoreError| e.to_string())?;

        if verifying_key != stored_pubkey {
            return Err("Provided private key does not match the stored public key.".to_string());
        }

        // 3. Regenerate signatures
        sign_constitutional_documents(&signing_key, &docs_dir).map_err(|e| e.to_string())?;

        tracing::info!("Identity repair successful. Signatures regenerated.");
        return Ok(());
    }

    Err("Invalid repair parameters: provide either private_key or reset=true".to_string())
}

#[tauri::command]
fn configure_email(
    state: tauri::State<AppState>,
    address: String,
    imap_host: String,
    imap_port: u16,
    smtp_host: String,
    smtp_port: u16,
    password: String,
) -> Result<(), String> {
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    let password_encrypted =
        Keyring::encrypt_bytes(password.as_bytes()).map_err(|e| e.to_string())?;
    config.email = Some(abigail_core::EmailConfig {
        address,
        imap_host,
        imap_port,
        smtp_host,
        smtp_port,
        password_encrypted,
    });
    config
        .save(&config.config_path())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn download_model(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<PathBuf, String> {
    // Get models_dir and drop the lock before await
    let models_dir = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        config.models_dir.clone()
    };
    std::fs::create_dir_all(&models_dir).map_err(|e| e.to_string())?;
    let downloader = abigail_capabilities::cognitive::ModelDownloader::new();
    let dest = downloader
        .download_to(&models_dir, |written, total_bytes| {
            let payload = serde_json::json!({ "written": written, "total": total_bytes });
            let _ = app.emit("download-progress", payload);
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(dest)
}

#[tauri::command]
fn set_api_key(state: tauri::State<AppState>, key: String) -> Result<(), String> {
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.openai_api_key = Some(key);
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    // Rebuild router using centralized logic (preserves Superego, uses correct provider)
    rebuild_router_with_superego(&state)
}

#[tauri::command]
async fn set_local_llm_url(state: tauri::State<'_, AppState>, url: String) -> Result<(), String> {
    let (local_url, ego_provider, ego_api_key, mode, superego_config) = {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.local_llm_base_url = if url.is_empty() {
            None
        } else {
            let normalized = validate_local_llm_url(&url).map_err(|e| e.to_string())?;
            Some(normalized)
        };
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
        let vault = state.secrets.lock().map_err(|e| e.to_string())?;
        let (ep, ek) = determine_ego_provider(&config, &vault);
        let se = extract_superego_config(&config);
        (
            config.local_llm_base_url.clone(),
            ep,
            ek,
            config.routing_mode,
            se,
        )
    };

    tracing::info!(
        "set_local_llm_url: rebuilding router with local_url={:?}, ego={:?}, mode={:?}",
        local_url,
        ego_provider,
        mode
    );

    // Rebuild the router with auto-detected model name (important for LM Studio)
    let mut new_router =
        IdEgoRouter::new_auto_detect(local_url, ego_provider.as_deref(), ego_api_key, mode).await;

    // Preserve Superego if configured
    if let Some((se_provider, se_key)) = superego_config {
        let superego = build_superego_llm_provider(&se_provider, &se_key);
        new_router = new_router.with_superego(superego);
        tracing::info!(
            "set_local_llm_url: Superego preserved (provider={})",
            se_provider
        );
    }

    let mut router = state.router.write().map_err(|e| e.to_string())?;
    *router = new_router;
    Ok(())
}

/// Configure the Superego (safety) layer.
/// Accepts a provider name ("openai", "anthropic") and API key.
/// Rebuilds the router with the Superego attached.
#[tauri::command]
fn set_superego_provider(
    state: tauri::State<AppState>,
    provider: String,
    key: String,
) -> Result<(), String> {
    // Store superego config in TrinityConfig
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        let trinity = config.trinity.get_or_insert_with(TrinityConfig::default);
        trinity.superego_provider = Some(provider.clone());
        trinity.superego_api_key = Some(key.clone());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    // Rebuild router with superego
    rebuild_router_with_superego(&state)?;
    Ok(())
}

/// Extract Superego provider name and key from TrinityConfig if configured.
fn extract_superego_config(config: &AppConfig) -> Option<(String, String)> {
    config.trinity.as_ref().and_then(|trinity| {
        match (&trinity.superego_provider, &trinity.superego_api_key) {
            (Some(provider), Some(key)) if !key.is_empty() => Some((provider.clone(), key.clone())),
            _ => None,
        }
    })
}

/// Build a Superego LLM provider from provider name and API key.
fn build_superego_llm_provider(
    provider: &str,
    key: &str,
) -> Arc<dyn abigail_capabilities::cognitive::LlmProvider> {
    match provider {
        "anthropic" => Arc::new(abigail_capabilities::cognitive::AnthropicProvider::new(
            key.to_string(),
        )),
        "perplexity" | "xai" | "google" => {
            if let Some(cp) =
                abigail_capabilities::cognitive::CompatibleProvider::from_name(provider)
            {
                Arc::new(
                    abigail_capabilities::cognitive::OpenAiCompatibleProvider::new(
                        cp,
                        key.to_string(),
                    ),
                )
            } else {
                Arc::new(abigail_capabilities::cognitive::OpenAiProvider::new(Some(
                    key.to_string(),
                )))
            }
        }
        _ => Arc::new(abigail_capabilities::cognitive::OpenAiProvider::new(Some(
            key.to_string(),
        ))),
    }
}

/// Rebuild the router from current config + vault state, attaching Superego if configured.
fn rebuild_router_with_superego(state: &AppState) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let vault = state.secrets.lock().map_err(|e| e.to_string())?;

    // Use centralized ego provider determination (TrinityConfig > config key > per-agent vault > hive vault)
    let (ego_name, ego_key) = {
        let (name, key) = determine_ego_provider(&config, &vault);
        if name.is_some() {
            (name, key)
        } else {
            // Fall back to hive-level vault
            let hive = state.hive_secrets.lock().map_err(|e| e.to_string())?;
            determine_ego_provider(&config, &hive)
        }
    };

    tracing::info!(
        "rebuild_router_with_superego: ego_provider={:?}, local_url={:?}, mode={:?}",
        ego_name,
        config.local_llm_base_url,
        config.routing_mode
    );

    let mut new_router = IdEgoRouter::new(
        config.local_llm_base_url.clone(),
        ego_name.as_deref(),
        ego_key,
        config.routing_mode,
    );

    // Attach Superego if configured in TrinityConfig (uses shared helper)
    if let Some((se_provider, se_key)) = extract_superego_config(&config) {
        let superego = build_superego_llm_provider(&se_provider, &se_key);
        new_router = new_router.with_superego(superego);
        tracing::info!("Superego attached: provider={}", se_provider);
    }

    drop(vault);
    drop(config);
    let router_arc = Arc::new(new_router.clone());
    let mut router = state.router.write().map_err(|e| e.to_string())?;
    *router = new_router;
    drop(router);

    // Keep subagent manager's router reference in sync
    if let Ok(mut mgr) = state.subagent_manager.write() {
        mgr.update_router(router_arc);
    }

    Ok(())
}

/// Status of the Id/Ego/Superego router for debugging and UI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterStatus {
    /// Id provider type: "candle_stub", "local_http", or "ollama"
    pub id_provider: String,
    /// Local LLM URL if configured
    pub id_url: Option<String>,
    /// Whether Ego (cloud) is configured
    pub ego_configured: bool,
    /// Which cloud provider backs Ego: "openai", "anthropic", or null
    pub ego_provider: Option<String>,
    /// Whether Superego (safety layer) is configured
    pub superego_configured: bool,
    /// Current routing mode: "ego_primary" or "id_primary"
    pub routing_mode: String,
}

#[tauri::command]
fn get_router_status(state: tauri::State<AppState>) -> Result<RouterStatus, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let vault = state.secrets.lock().map_err(|e| e.to_string())?;

    let id_provider = {
        let router = state.router.read().map_err(|e| e.to_string())?;
        if router.is_using_http_provider() {
            "local_http".to_string()
        } else {
            "candle_stub".to_string()
        }
    };

    let (ego_provider, ego_key) = determine_ego_provider(&config, &vault);

    Ok(RouterStatus {
        id_provider,
        id_url: config.local_llm_base_url.clone(),
        ego_configured: ego_key.is_some(),
        ego_provider: ego_provider,
        superego_configured: state
            .router
            .read()
            .map(|r| r.has_superego())
            .unwrap_or(false),
        routing_mode: format!("{:?}", config.routing_mode).to_lowercase(),
    })
}

#[tauri::command]
fn complete_birth(state: tauri::State<AppState>) -> Result<(), String> {
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;
    b.complete_birth().map_err(|e| e.to_string())?;
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    config.birth_complete = true;
    config
        .save(&config.config_path())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn list_skills(
    state: tauri::State<AppState>,
) -> Result<Vec<abigail_skills::SkillManifest>, String> {
    state.registry.list().map_err(|e| e.to_string())
}

#[tauri::command]
fn list_discovered_skills(
    state: tauri::State<AppState>,
) -> Result<Vec<abigail_skills::SkillManifest>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let paths = vec![config.data_dir.join("skills")];
    Ok(abigail_skills::SkillRegistry::discover(&paths))
}

#[tauri::command]
fn list_tools(
    state: tauri::State<AppState>,
    skill_id: String,
) -> Result<Vec<abigail_skills::ToolDescriptor>, String> {
    let id = abigail_skills::SkillId(skill_id);
    let (skill, _) = state.registry.get_skill(&id).map_err(|e| e.to_string())?;
    Ok(skill.tools())
}

#[tauri::command]
async fn execute_tool(
    state: tauri::State<'_, AppState>,
    skill_id: String,
    tool_name: String,
    params: HashMap<String, serde_json::Value>,
) -> Result<abigail_skills::ToolOutput, String> {
    {
        let config = state.config.read().map_err(|e| e.to_string())?;
        if !config.approved_skill_ids.is_empty() && !config.approved_skill_ids.contains(&skill_id) {
            return Err(format!(
                "Skill {} is not approved for execution. Approve it in settings or install it first.",
                skill_id
            ));
        }
    }
    let id = abigail_skills::SkillId(skill_id);
    let tool_params = ToolParams { values: params };
    state
        .executor
        .execute(&id, &tool_name, tool_params)
        .await
        .map_err(|e| e.to_string())
}

// ── MCP (Model Context Protocol) ────────────────────────────────────

#[tauri::command]
fn get_mcp_servers(state: tauri::State<AppState>) -> Result<Vec<McpServerDefinition>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.mcp_servers.clone())
}

#[tauri::command]
async fn mcp_list_tools(
    state: tauri::State<'_, AppState>,
    server_id: String,
) -> Result<Vec<McpTool>, String> {
    let url = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let server = config
            .mcp_servers
            .iter()
            .find(|s| s.id == server_id)
            .ok_or_else(|| format!("MCP server not found: {}", server_id))?;
        if server.transport != "http" {
            return Err("Only HTTP transport is supported for MCP list_tools".to_string());
        }
        server.command_or_url.clone()
    }; // guard dropped here
    let client = HttpMcpClient::new(url);
    client.initialize().await.map_err(|e| e.to_string())?;
    client.list_tools_impl().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn execute_mcp_tool(
    state: tauri::State<'_, AppState>,
    server_id: String,
    tool_name: String,
    params: HashMap<String, serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let url = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let server = config
            .mcp_servers
            .iter()
            .find(|s| s.id == server_id)
            .ok_or_else(|| format!("MCP server not found: {}", server_id))?;
        if server.transport != "http" {
            return Err("Only HTTP transport is supported for MCP tool execution".to_string());
        }
        server.command_or_url.clone()
    }; // guard dropped here
    let client = HttpMcpClient::new(url);
    let args = serde_json::to_value(&params).map_err(|e| e.to_string())?;
    client
        .call_tool_impl(&tool_name, args)
        .await
        .map_err(|e| e.to_string())
}

/// Fetch MCP App UI content (e.g. ui:// resource) for sandboxed iframe rendering.
#[tauri::command]
async fn get_mcp_app_content(
    state: tauri::State<'_, AppState>,
    server_id: String,
    resource_uri: String,
) -> Result<String, String> {
    let url = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let server = config
            .mcp_servers
            .iter()
            .find(|s| s.id == server_id)
            .ok_or_else(|| format!("MCP server not found: {}", server_id))?;
        if server.transport != "http" {
            return Err("Only HTTP transport is supported for MCP Apps".to_string());
        }
        server.command_or_url.clone()
    }; // guard dropped here
    let client = HttpMcpClient::new(url);
    client
        .read_resource(&resource_uri)
        .await
        .map_err(|e| e.to_string())
}

// ── Skill install / approval ────────────────────────────────────────

#[tauri::command]
fn list_approved_skills(state: tauri::State<AppState>) -> Result<Vec<String>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.approved_skill_ids.clone())
}

#[tauri::command]
fn install_skill(state: tauri::State<AppState>, package_path: String) -> Result<String, String> {
    let path = Path::new(&package_path);
    if !path.is_dir() {
        return Err("Package path must be a directory".to_string());
    }
    let manifest_path = path.join("skill.toml");
    if !manifest_path.is_file() {
        return Err("Directory must contain skill.toml".to_string());
    }
    let manifest =
        abigail_skills::SkillManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let skill_id = manifest.id.0.clone();

    let data_dir = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        config.data_dir.clone()
    };
    let skills_dir = data_dir.join("skills");
    std::fs::create_dir_all(&skills_dir).map_err(|e| e.to_string())?;
    let dest = skills_dir.join(&skill_id);
    if dest.exists() {
        return Err(format!("Skill {} is already installed", skill_id));
    }
    copy_dir_all(path, &dest).map_err(|e| e.to_string())?;

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        if !config.approved_skill_ids.contains(&skill_id) {
            config.approved_skill_ids.push(skill_id.clone());
        }
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    skill_audit_log(&data_dir, "install", &format!("skill_id={}", skill_id));
    Ok(skill_id)
}

#[tauri::command]
fn uninstall_skill(state: tauri::State<AppState>, skill_id: String) -> Result<(), String> {
    let data_dir = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        config.data_dir.clone()
    };
    let skill_path = data_dir.join("skills").join(&skill_id);
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.approved_skill_ids.retain(|id| id != &skill_id);
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }
    if skill_path.exists() {
        std::fs::remove_dir_all(&skill_path).map_err(|e| e.to_string())?;
    }
    skill_audit_log(&data_dir, "uninstall", &format!("skill_id={}", skill_id));
    Ok(())
}

#[tauri::command]
fn approve_skill(state: tauri::State<AppState>, skill_id: String) -> Result<(), String> {
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    if !config.approved_skill_ids.contains(&skill_id) {
        config.approved_skill_ids.push(skill_id.clone());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
        skill_audit_log(
            &config.data_dir,
            "approve",
            &format!("skill_id={}", skill_id),
        );
    }
    Ok(())
}

// ── Secrets Management ──────────────────────────────────────────────

/// Reserved provider names for API keys (must match validation.rs known providers).
const ALLOWED_PROVIDER_SECRET_KEYS: &[&str] = &["openai", "anthropic", "xai", "google", "tavily"];

/// Returns the set of allowed secret keys: reserved provider names + skill-declared secret names.
fn allowed_secret_keys(
    _registry: &SkillRegistry,
    skill_paths: &[PathBuf],
) -> std::collections::HashSet<String> {
    let mut allowed: std::collections::HashSet<String> = ALLOWED_PROVIDER_SECRET_KEYS
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let manifests = abigail_skills::SkillRegistry::discover(skill_paths);
    for m in manifests {
        for s in &m.secrets {
            allowed.insert(s.name.clone());
        }
    }
    allowed
}

#[tauri::command]
fn check_secret(state: tauri::State<AppState>, key: String) -> Result<bool, String> {
    let (allowed, exists) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let paths = vec![config.data_dir.join("skills")];
        let allowed = allowed_secret_keys(&state.registry, &paths);
        let vault = state.secrets.lock().map_err(|e| e.to_string())?;
        (allowed, vault.exists(&key))
    };
    if !allowed.contains(&key) {
        return Err("Secret key not allowed. Use a reserved provider name (e.g. openai, anthropic) or a skill-declared secret name.".to_string());
    }
    Ok(exists)
}

#[tauri::command]
fn store_secret(state: tauri::State<AppState>, key: String, value: String) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let paths = vec![config.data_dir.join("skills")];
    let allowed = allowed_secret_keys(&state.registry, &paths);
    if !allowed.contains(&key) {
        return Err("Secret key not allowed. Use a reserved provider name (e.g. openai, anthropic) or a skill-declared secret name.".to_string());
    }
    let mut vault = state.secrets.lock().map_err(|e| e.to_string())?;
    vault.set_secret(&key, &value);
    vault.save().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn remove_secret(state: tauri::State<AppState>, key: String) -> Result<bool, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let paths = vec![config.data_dir.join("skills")];
    let allowed = allowed_secret_keys(&state.registry, &paths);
    if !allowed.contains(&key) {
        return Err(
            "Secret key not allowed. Use a reserved provider name or a skill-declared secret name."
                .to_string(),
        );
    }
    let mut vault = state.secrets.lock().map_err(|e| e.to_string())?;
    let removed = vault.remove_secret(&key);
    if removed {
        vault.save().map_err(|e| e.to_string())?;
    }
    Ok(removed)
}

#[tauri::command]
fn list_missing_skill_secrets(
    state: tauri::State<AppState>,
) -> Result<Vec<MissingSkillSecret>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let paths = vec![config.data_dir.join("skills")];
    Ok(state.registry.list_all_missing_secrets(&paths))
}

// ── Chat ────────────────────────────────────────────────────────────

/// Build tool definitions for the chat command, including registered skills.
fn chat_tool_definitions(
    registry: &SkillRegistry,
) -> Vec<abigail_capabilities::cognitive::ToolDefinition> {
    let mut tools = Vec::new();

    // Built-in: store_provider_key
    let schema = abigail_capabilities::cognitive::update_provider_key_schema();
    tools.push(abigail_capabilities::cognitive::ToolDefinition {
        name: schema["name"]
            .as_str()
            .unwrap_or("store_provider_key")
            .to_string(),
        description: schema["description"].as_str().unwrap_or("").to_string(),
        parameters: schema["parameters"].clone(),
    });

    // Skill-provided tools
    if let Ok(manifests) = registry.list() {
        for manifest in &manifests {
            if let Ok((skill, _)) = registry.get_skill(&manifest.id) {
                for td in skill.tools() {
                    tools.push(abigail_capabilities::cognitive::ToolDefinition {
                        name: td.name.clone(),
                        description: td.description.clone(),
                        parameters: td.parameters.clone(),
                    });
                }
            }
        }
    }

    tools
}

/// Auto-detect provider from API key prefix.
fn detect_provider_from_prefix(key: &str) -> Option<&'static str> {
    if key.starts_with("sk-ant-") {
        Some("anthropic")
    } else if key.starts_with("sk-") {
        Some("openai")
    } else if key.starts_with("xai-") {
        Some("xai")
    } else if key.starts_with("AIza") {
        Some("google")
    } else if key.starts_with("tvly-") {
        Some("tavily")
    } else {
        None
    }
}

/// Execute a tool call from the LLM and return the result string.
async fn execute_tool_call(
    state: &tauri::State<'_, AppState>,
    app: &tauri::AppHandle,
    tool_call: &abigail_capabilities::cognitive::ToolCall,
) -> String {
    match tool_call.name.as_str() {
        "update_provider_key" | "store_provider_key" => {
            let args: serde_json::Value = match serde_json::from_str(&tool_call.arguments) {
                Ok(v) => v,
                Err(e) => return format!("Error parsing arguments: {}", e),
            };
            let raw_provider = args["provider"].as_str().unwrap_or("");
            let key = args["key"].as_str().unwrap_or("");

            if key.is_empty() {
                return "Error: key is required".to_string();
            }

            // Auto-detect provider from key prefix if provider is "auto" or empty
            let provider = if raw_provider.is_empty() || raw_provider == "auto" {
                match detect_provider_from_prefix(key) {
                    Some(detected) => detected,
                    None => return "Error: could not auto-detect provider from key prefix. Please specify the provider explicitly.".to_string(),
                }
            } else {
                raw_provider
            };

            // Validate the key first
            if let Err(e) =
                abigail_capabilities::cognitive::validation::validate_api_key(provider, key).await
            {
                return format!(
                    "API key validation failed: {}. Please check the key and try again.",
                    e
                );
            }

            // Store in per-agent secrets vault
            {
                let mut vault = match state.secrets.lock() {
                    Ok(v) => v,
                    Err(e) => return format!("Error accessing vault: {}", e),
                };
                vault.set_secret(provider, key);
                if let Err(e) = vault.save() {
                    return format!("Error saving key: {}", e);
                }
            }

            // Also store in hive-level vault so all agents can access it
            if let Ok(mut hive) = state.hive_secrets.lock() {
                hive.set_secret(provider, key);
                let _ = hive.save();
            }

            // For known Ego providers, persist in TrinityConfig and rebuild router
            if matches!(
                provider,
                "openai" | "anthropic" | "perplexity" | "xai" | "google"
            ) {
                if let Ok(mut config) = state.config.write() {
                    if provider == "openai" {
                        config.openai_api_key = Some(key.to_string());
                    }
                    // Persist ego provider in TrinityConfig for restart resilience
                    let trinity = config.trinity.get_or_insert_with(Default::default);
                    trinity.ego_provider = Some(provider.to_string());
                    trinity.ego_api_key = Some(key.to_string());
                    let _ = config.save(&config.config_path());
                    drop(config);
                }
                // Use centralized rebuild (preserves Superego)
                let _ = rebuild_router_with_superego(state);
            }

            format!(
                "Successfully validated and stored {} API key in secure vault.",
                provider
            )
        }
        "recommend_crystallize" => {
            let args: serde_json::Value = match serde_json::from_str(&tool_call.arguments) {
                Ok(v) => v,
                Err(e) => return format!("Error parsing arguments: {}", e),
            };
            let name = args["name"].as_str().unwrap_or("");
            let purpose = args["purpose"].as_str().unwrap_or("");
            let personality = args["personality"].as_str().unwrap_or("");

            if name.is_empty() || purpose.is_empty() || personality.is_empty() {
                return "Error: name, purpose, and personality are all required".to_string();
            }

            format!(
                "Crystallization recommended with name='{}', purpose='{}', personality='{}'.",
                name, purpose, personality
            )
        }
        other => {
            // Check registered skills for matching tool name
            let _ = app.emit(
                "chat-status",
                serde_json::json!({
                    "status": "tool_executing",
                    "tool": other,
                }),
            );

            // Search skills for a tool with this name
            if let Ok(manifests) = state.registry.list() {
                for manifest in &manifests {
                    if let Ok((skill, _)) = state.registry.get_skill(&manifest.id) {
                        if skill.tools().iter().any(|t| t.name == other) {
                            // Parse arguments into ToolParams
                            let args: serde_json::Value =
                                match serde_json::from_str(&tool_call.arguments) {
                                    Ok(v) => v,
                                    Err(e) => return format!("Error parsing arguments: {}", e),
                                };

                            let mut params = ToolParams::new();
                            if let Some(obj) = args.as_object() {
                                for (k, v) in obj {
                                    params.values.insert(k.clone(), v.clone());
                                }
                            }

                            match state.executor.execute(&manifest.id, other, params).await {
                                Ok(output) => {
                                    if output.success {
                                        // Extract formatted text for LLM consumption
                                        if let Some(ref data) = output.data {
                                            if let Some(formatted) =
                                                data.get("formatted").and_then(|f| f.as_str())
                                            {
                                                return formatted.to_string();
                                            }
                                            return data.to_string();
                                        }
                                        return "Tool executed successfully".to_string();
                                    } else {
                                        return output
                                            .error
                                            .unwrap_or_else(|| "Tool failed".to_string());
                                    }
                                }
                                Err(e) => return format!("Tool error: {}", e),
                            }
                        }
                    }
                }
            }

            format!("Unknown tool: {}", other)
        }
    }
}

/// Parse text-based tool calls from LLM output.
/// Supports patterns like:
/// - ```tool_request\n{"name": "...", "arguments": {...}}\n```
/// - ```json\n{"name": "...", "arguments": {...}}\n```
/// - [TOOL_CALL]{"name": "...", "arguments": {...}}[/TOOL_CALL]
/// - Inline JSON with tool structure
/// Returns a list of parsed tool calls and the remaining text (without tool blocks).
fn parse_text_tool_calls(
    content: &str,
) -> (Vec<abigail_capabilities::cognitive::ToolCall>, String) {
    let mut tool_calls = Vec::new();
    let mut cleaned_content = content.to_string();

    // Helper to try parsing a JSON string as a tool call
    let try_parse_tool = |json_str: &str| -> Option<abigail_capabilities::cognitive::ToolCall> {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
            if let Some(name) = parsed.get("name").and_then(|n| n.as_str()) {
                let arguments = if let Some(args) = parsed.get("arguments") {
                    args.to_string()
                } else {
                    "{}".to_string()
                };
                return Some(abigail_capabilities::cognitive::ToolCall {
                    id: String::new(), // Will be assigned later
                    name: name.to_string(),
                    arguments,
                });
            }
        }
        None
    };

    // Pattern 1: ```tool_request\n...\n``` or ```json\n...\n``` with tool structure
    // Use greedy matching for nested braces
    let code_block_re = Regex::new(r"```(?:tool_request|json)?\s*\n([\s\S]*?)\n```").unwrap();
    for cap in code_block_re.captures_iter(content) {
        if let Some(json_match) = cap.get(1) {
            let json_str = json_match.as_str().trim();
            if let Some(mut tc) = try_parse_tool(json_str) {
                tc.id = format!("text_call_{}", tool_calls.len());
                tool_calls.push(tc);
            }
        }
    }
    cleaned_content = code_block_re.replace_all(&cleaned_content, "").to_string();

    // Pattern 2: [TOOL_CALL]...[/TOOL_CALL]
    let tag_re = Regex::new(r"\[TOOL_CALL\]([\s\S]*?)\[/TOOL_CALL\]").unwrap();
    for cap in tag_re.captures_iter(content) {
        if let Some(json_match) = cap.get(1) {
            let json_str = json_match.as_str().trim();
            if let Some(mut tc) = try_parse_tool(json_str) {
                if !tool_calls.iter().any(|t| t.name == tc.name) {
                    tc.id = format!("text_call_{}", tool_calls.len());
                    tool_calls.push(tc);
                }
            }
        }
    }
    cleaned_content = tag_re.replace_all(&cleaned_content, "").to_string();

    // Pattern 3: Inline JSON that looks like a tool call (with name and arguments fields)
    // This catches cases where the LLM outputs JSON without code blocks
    let inline_json_re =
        Regex::new(r#"\{[^{}]*"name"\s*:\s*"[^"]+"\s*,\s*"arguments"\s*:\s*\{[^{}]*\}[^{}]*\}"#)
            .unwrap();
    for mat in inline_json_re.find_iter(content) {
        let json_str = mat.as_str();
        if let Some(mut tc) = try_parse_tool(json_str) {
            if !tool_calls.iter().any(|t| t.name == tc.name) {
                tc.id = format!("text_call_{}", tool_calls.len());
                tool_calls.push(tc);
                cleaned_content = cleaned_content.replace(json_str, "");
            }
        }
    }

    // Clean up [END_TOOL_REQUEST] tags and extra whitespace
    let end_tag_re = Regex::new(r"\[END_TOOL_REQUEST\]").unwrap();
    cleaned_content = end_tag_re.replace_all(&cleaned_content, "").to_string();
    cleaned_content = cleaned_content.trim().to_string();

    (tool_calls, cleaned_content)
}

#[tauri::command]
async fn chat(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    message: String,
    target: Option<String>,
) -> Result<String, String> {
    // Build system prompt and gather config before async boundary
    let (store, router, system_prompt) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let store = MemoryStore::open_with_config(&*config).map_err(|e| e.to_string())?;
        let prompt =
            abigail_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        drop(config);
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (store, router, prompt)
    };

    // Build messages with system prompt
    let mut messages = vec![
        abigail_capabilities::cognitive::Message::new("system", &system_prompt),
        abigail_capabilities::cognitive::Message::new("user", &message),
    ];

    let target_mode = target.as_deref().unwrap_or("EGO");
    let tools = chat_tool_definitions(&state.registry);

    // First request — route based on target
    let response = if target_mode == "ID" {
        router
            .id_only(messages.clone())
            .await
            .map_err(|e| e.to_string())?
    } else {
        router
            .route_with_tools(messages.clone(), tools.clone())
            .await
            .map_err(|e| e.to_string())?
    };

    let final_content = if target_mode != "ID" {
        if let Some(ref tool_calls) = response.tool_calls {
            // Execute each tool call and collect results
            let mut tool_results = Vec::new();
            for tc in tool_calls {
                let result = execute_tool_call(&state, &app, tc).await;
                tool_results.push((tc.clone(), result));
            }

            // Build follow-up: original messages + assistant with tool_calls + tool results
            messages.push(abigail_capabilities::cognitive::Message {
                role: "assistant".to_string(),
                content: response.content.clone(),
                tool_call_id: None,
                tool_calls: Some(tool_calls.clone()),
            });

            for (tc, result) in &tool_results {
                messages.push(abigail_capabilities::cognitive::Message::tool_result(
                    &tc.id, result,
                ));
            }

            // Send follow-up for final natural-language response
            let follow_up = router
                .route_with_tools(messages, tools)
                .await
                .map_err(|e| e.to_string())?;
            follow_up.content
        } else {
            response.content
        }
    } else {
        // ID mode: check for text-based tool calls (for local LLMs without native function calling)
        let (text_tool_calls, cleaned_content) = parse_text_tool_calls(&response.content);

        if !text_tool_calls.is_empty() {
            // Execute text-based tool calls
            let mut tool_results = Vec::new();
            for tc in &text_tool_calls {
                let result = execute_tool_call(&state, &app, tc).await;
                tool_results.push((tc.name.clone(), result));
            }

            // Build follow-up with tool results for final response
            messages.push(abigail_capabilities::cognitive::Message::new(
                "assistant",
                &cleaned_content,
            ));

            // Add tool results as a system message so the LLM knows what happened
            let results_summary: String = tool_results
                .iter()
                .map(|(name, result)| format!("[Tool '{}' result]: {}", name, result))
                .collect::<Vec<_>>()
                .join("\n");
            messages.push(abigail_capabilities::cognitive::Message::new(
                "system",
                &results_summary,
            ));

            // Get follow-up response from Id
            let follow_up = router.id_only(messages).await.map_err(|e| e.to_string())?;
            follow_up.content
        } else {
            response.content
        }
    };

    // Store memory
    let memory = Memory::ephemeral(format!("user: {} | assistant: {}", message, final_content));
    let _ = store.insert_memory(&memory);
    Ok(final_content)
}

/// Streaming version of chat: emits "chat-token" events as tokens arrive.
/// Falls back to non-streaming for tool-calling follow-ups.
#[tauri::command]
async fn chat_stream(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    message: String,
    target: Option<String>,
) -> Result<String, String> {
    use abigail_capabilities::cognitive::StreamEvent;

    let (store, router, system_prompt) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let store = MemoryStore::open_with_config(&*config).map_err(|e| e.to_string())?;
        let prompt =
            abigail_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        drop(config);
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (store, router, prompt)
    };

    let mut messages = vec![
        abigail_capabilities::cognitive::Message::new("system", &system_prompt),
        abigail_capabilities::cognitive::Message::new("user", &message),
    ];

    let target_mode = target.as_deref().unwrap_or("EGO");
    let tools = chat_tool_definitions(&state.registry);

    tracing::debug!(
        "chat_stream: target_mode={}, has_tools={}",
        target_mode,
        !tools.is_empty()
    );

    // For simple (no-tool) streaming, use the streaming path.
    // For tool-calling, we do a non-streaming initial request then stream the follow-up.
    let final_content = if target_mode == "ID" {
        tracing::debug!("chat_stream: using Id-only streaming path");
        // Id-only: stream directly (no tool calling)
        let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
        let app_clone = app.clone();

        // Spawn a task to forward stream events to the frontend
        let forward_handle = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    StreamEvent::Token(token) => {
                        let _ = app_clone.emit("chat-token", serde_json::json!({ "token": token }));
                    }
                    StreamEvent::Done(_) => {
                        let _ = app_clone.emit("chat-token", serde_json::json!({ "done": true }));
                    }
                }
            }
        });

        let _request = abigail_capabilities::cognitive::CompletionRequest::simple(messages.clone());
        let response = router
            .route_stream(messages, tx)
            .await
            .map_err(|e| e.to_string())?;
        let _ = forward_handle.await;
        response.content
    } else {
        tracing::info!(
            "chat_stream: using Ego streaming-first path with {} tools",
            tools.len()
        );

        // Ego mode: stream directly with tools. The router's route_stream_with_tools()
        // handles streaming with inline tool call accumulation and fallback chain
        // (Ego stream → Id stream → non-streaming).
        let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
        let app_clone = app.clone();

        let forward_handle = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    StreamEvent::Token(token) => {
                        let _ =
                            app_clone.emit("chat-token", serde_json::json!({ "token": token }));
                    }
                    StreamEvent::Done(_) => {
                        let _ =
                            app_clone.emit("chat-token", serde_json::json!({ "done": true }));
                    }
                }
            }
        });

        let response = router
            .route_stream_with_tools(messages.clone(), tools.clone(), tx)
            .await
            .map_err(|e| {
                tracing::warn!("chat_stream: Ego streaming failed: {}", e);
                e.to_string()
            })?;
        let _ = forward_handle.await;

        // If the streaming response included tool calls, execute them and stream a follow-up
        if let Some(ref tool_calls) = response.tool_calls {
            tracing::info!(
                "chat_stream: Ego returned {} tool call(s), executing",
                tool_calls.len()
            );
            let mut tool_results = Vec::new();
            for tc in tool_calls {
                let result = execute_tool_call(&state, &app, tc).await;
                tool_results.push((tc.clone(), result));
            }

            messages.push(abigail_capabilities::cognitive::Message {
                role: "assistant".to_string(),
                content: response.content.clone(),
                tool_call_id: None,
                tool_calls: Some(tool_calls.clone()),
            });

            for (tc, result) in &tool_results {
                messages.push(abigail_capabilities::cognitive::Message::tool_result(
                    &tc.id, result,
                ));
            }

            // Stream the follow-up response after tool execution
            let (tx2, mut rx2) = tokio::sync::mpsc::channel::<StreamEvent>(64);
            let app_clone2 = app.clone();

            let forward_handle2 = tokio::spawn(async move {
                while let Some(event) = rx2.recv().await {
                    match event {
                        StreamEvent::Token(token) => {
                            let _ = app_clone2
                                .emit("chat-token", serde_json::json!({ "token": token }));
                        }
                        StreamEvent::Done(_) => {
                            let _ = app_clone2
                                .emit("chat-token", serde_json::json!({ "done": true }));
                        }
                    }
                }
            });

            tracing::info!("chat_stream: streaming follow-up after tool execution");
            let follow_up = router
                .route_stream(messages, tx2)
                .await
                .map_err(|e| e.to_string())?;
            let _ = forward_handle2.await;
            follow_up.content
        } else {
            tracing::info!("chat_stream: Ego response complete (no tool calls)");
            response.content
        }
    };

    // Store memory
    let memory = Memory::ephemeral(format!("user: {} | assistant: {}", message, final_content));
    let _ = store.insert_memory(&memory);
    Ok(final_content)
}

/// MVP shortcut: skip email and model download, go directly to Emergence stage.
/// Used for streamlined first-run experience.
#[tauri::command]
fn skip_to_life_for_mvp(state: tauri::State<AppState>) -> Result<(), String> {
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;
    b.skip_to_life_for_mvp();
    Ok(())
}

// ── New Birth Flow Commands ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedLlm {
    pub name: String,
    pub url: String,
    pub reachable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub detected: Vec<DetectedLlm>,
}

/// Auto-detect local LLM servers on common ports.
#[tauri::command]
async fn probe_local_llm() -> Result<ProbeResult, String> {
    let candidates = vec![
        ("Ollama", "http://localhost:11434"),
        ("LM Studio", "http://localhost:1234"),
    ];

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .connect_timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;

    let mut detected = Vec::new();

    for (name, url) in candidates {
        let probe_url = format!("{}/v1/models", url);
        let reachable = client.get(&probe_url).send().await.is_ok();
        detected.push(DetectedLlm {
            name: name.to_string(),
            url: url.to_string(),
            reachable,
        });
    }

    Ok(ProbeResult { detected })
}

/// Set local LLM URL during birth, heartbeat it, and advance past Ignition.
#[tauri::command]
async fn set_local_llm_during_birth(
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<bool, String> {
    let normalized_url = validate_local_llm_url(&url).map_err(|e| e.to_string())?;
    // Set the URL in config
    let (ego_provider, ego_api_key, mode) = {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.local_llm_base_url = Some(normalized_url.clone());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
        let vault = state.secrets.lock().map_err(|e| e.to_string())?;
        let (ep, ek) = determine_ego_provider(&config, &vault);
        (ep, ek, config.routing_mode)
    };

    // Rebuild router with auto-detected model name (important for LM Studio)
    let new_router = IdEgoRouter::new_auto_detect(
        Some(normalized_url.clone()),
        ego_provider.as_deref(),
        ego_api_key,
        mode,
    )
    .await;
    {
        let mut router = state.router.write().map_err(|e| e.to_string())?;
        *router = new_router;
    }

    // Heartbeat the new provider
    let router = state.router.read().map_err(|e| e.to_string())?.clone();
    let heartbeat_ok = router.heartbeat().await.is_ok();

    if heartbeat_ok {
        // Also update birth orchestrator config
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        if let Some(b) = birth.as_mut() {
            b.config_mut().local_llm_base_url = Some(normalized_url);
            let _ = b.advance_to_connectivity(); // Ignore error if already past this stage
        }
    }

    Ok(heartbeat_ok)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BirthChatResponse {
    pub message: String,
    pub stage: String,
    pub action: Option<BirthAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BirthAction {
    RequestApiKey { provider: String },
    KeyStored { provider: String, validated: bool },
    SoulReady { preview: String },
    StageComplete,
}

/// Stage-aware chat during birth, routed exclusively through local LLM.
/// Supports text-based tool calls for LLMs without native function calling.
#[tauri::command]
async fn birth_chat(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    message: String,
) -> Result<BirthChatResponse, String> {
    // Get stored providers for context-aware prompt
    let stored_providers: Vec<String> = {
        let vault = state.secrets.lock().map_err(|e| e.to_string())?;
        vault
            .list_providers()
            .iter()
            .map(|s| s.to_string())
            .collect()
    };

    // Get stage and system prompt with context
    let (stage, system_prompt) = {
        let birth = state.birth.read().map_err(|e| e.to_string())?;
        let b = birth.as_ref().ok_or("Birth not started")?;
        let stage = b.current_stage();
        let prompt =
            abigail_birth::prompts::system_prompt_for_stage_with_context(stage, &stored_providers)
                .unwrap_or_else(|| "You are Abigail, a newborn AI agent.".to_string());
        (stage, prompt)
    };

    // Record user message
    {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        let b = birth.as_mut().ok_or("Birth not started")?;
        b.add_message("user", &message);
    }

    // Build messages array with system prompt + conversation history
    let mut messages = {
        let birth = state.birth.read().map_err(|e| e.to_string())?;
        let b = birth.as_ref().ok_or("Birth not started")?;
        let mut msgs = vec![abigail_capabilities::cognitive::Message::new(
            "system",
            &system_prompt,
        )];
        for (role, content) in b.get_conversation() {
            msgs.push(abigail_capabilities::cognitive::Message::new(role, content));
        }
        msgs
    };

    // Route through Ego if available, otherwise Id only
    let router = state.router.read().map_err(|e| e.to_string())?.clone();
    let response = if router.has_ego() {
        router
            .route(messages.clone())
            .await
            .map_err(|e| e.to_string())?
    } else {
        router
            .id_only(messages.clone())
            .await
            .map_err(|e| e.to_string())?
    };

    // Check for text-based tool calls (for local LLMs without native function calling)
    let (text_tool_calls, cleaned_content) = parse_text_tool_calls(&response.content);

    // Track actions to return to frontend
    let mut action: Option<BirthAction> = None;

    let final_content = if !text_tool_calls.is_empty() {
        // Execute text-based tool calls
        let mut tool_results = Vec::new();
        for tc in &text_tool_calls {
            let result = execute_tool_call(&state, &app, tc).await;

            // Detect successful store_provider_key calls
            if matches!(
                tc.name.as_str(),
                "store_provider_key" | "update_provider_key"
            ) && result.starts_with("Successfully")
            {
                let args: serde_json::Value =
                    serde_json::from_str(&tc.arguments).unwrap_or_default();
                let raw_provider = args["provider"].as_str().unwrap_or("unknown");
                let key = args["key"].as_str().unwrap_or("");
                // Resolve auto-detected provider for the action
                let resolved = if raw_provider == "auto" || raw_provider.is_empty() {
                    detect_provider_from_prefix(key)
                        .unwrap_or(raw_provider)
                        .to_string()
                } else {
                    raw_provider.to_string()
                };
                action = Some(BirthAction::KeyStored {
                    provider: resolved,
                    validated: true,
                });
            }

            // Detect recommend_crystallize calls (Issue 3)
            if tc.name == "recommend_crystallize" {
                let args: serde_json::Value =
                    serde_json::from_str(&tc.arguments).unwrap_or_default();
                let name = args["name"].as_str().unwrap_or("").to_string();
                let purpose = args["purpose"].as_str().unwrap_or("").to_string();
                let personality = args["personality"].as_str().unwrap_or("").to_string();
                if !name.is_empty() && !purpose.is_empty() && !personality.is_empty() {
                    let preview = serde_json::json!({
                        "name": name,
                        "purpose": purpose,
                        "personality": personality,
                    })
                    .to_string();
                    action = Some(BirthAction::SoulReady { preview });
                }
            }

            tool_results.push((tc.name.clone(), result));
        }

        // Build follow-up with tool results
        messages.push(abigail_capabilities::cognitive::Message::new(
            "assistant",
            &cleaned_content,
        ));

        // Add tool results as a system message
        let results_summary: String = tool_results
            .iter()
            .map(|(name, result)| format!("[Tool '{}' executed]: {}", name, result))
            .collect::<Vec<_>>()
            .join("\n");
        messages.push(abigail_capabilities::cognitive::Message::new(
            "system",
            &results_summary,
        ));

        // Re-read router (tool execution may have rebuilt it with Ego)
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        let follow_up = if router.has_ego() {
            router.route(messages).await.map_err(|e| e.to_string())?
        } else {
            router.id_only(messages).await.map_err(|e| e.to_string())?
        };

        // Record the full exchange in birth conversation (redact API keys)
        {
            let mut birth = state.birth.write().map_err(|e| e.to_string())?;
            let b = birth.as_mut().ok_or("Birth not started")?;
            b.add_message("assistant", &redact_api_keys(&cleaned_content));
            b.add_message("system", &redact_api_keys(&results_summary));
            b.add_message("assistant", &redact_api_keys(&follow_up.content));
        }

        follow_up.content
    } else {
        // No tool calls - record response normally
        {
            let mut birth = state.birth.write().map_err(|e| e.to_string())?;
            let b = birth.as_mut().ok_or("Birth not started")?;
            b.add_message("assistant", &redact_api_keys(&response.content));
        }
        response.content
    };

    Ok(BirthChatResponse {
        message: redact_api_keys(&final_content),
        stage: stage.name().to_string(),
        action,
    })
}

/// Result of storing a provider API key with optional validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreKeyResult {
    pub success: bool,
    pub provider: String,
    pub validated: bool,
    pub error: Option<String>,
}

/// Store a provider API key in the vault during Connectivity.
/// Validates the key first if `validate` is true (default).
#[tauri::command]
async fn store_provider_key(
    state: tauri::State<'_, AppState>,
    provider: String,
    key: String,
    validate: Option<bool>,
) -> Result<StoreKeyResult, String> {
    let should_validate = validate.unwrap_or(true);

    // Auto-detect provider from key prefix if provider is "auto" or empty
    let provider = if provider.is_empty() || provider == "auto" {
        match detect_provider_from_prefix(&key) {
            Some(detected) => detected.to_string(),
            None => {
                return Ok(StoreKeyResult {
                    success: false,
                    provider,
                    validated: false,
                    error: Some(
                        "Could not auto-detect provider from key prefix. Please specify the provider explicitly.".to_string(),
                    ),
                });
            }
        }
    } else {
        provider
    };

    // Validate if requested
    if should_validate {
        if let Err(e) =
            abigail_capabilities::cognitive::validation::validate_api_key(&provider, &key).await
        {
            return Ok(StoreKeyResult {
                success: false,
                provider,
                validated: false,
                error: Some(e.to_string()),
            });
        }
    }

    // Store in per-agent secrets vault
    {
        let mut vault = state.secrets.lock().map_err(|e| e.to_string())?;
        vault.set_secret(&provider, &key);
        vault.save().map_err(|e| e.to_string())?;
    }

    // Also store in hive-level vault so all agents can access it
    {
        let mut hive = state.hive_secrets.lock().map_err(|e| e.to_string())?;
        hive.set_secret(&provider, &key);
        let _ = hive.save();
    }

    // For known Ego providers, update config and rebuild router (preserving Superego)
    if matches!(
        provider.as_str(),
        "openai" | "anthropic" | "perplexity" | "xai" | "google"
    ) {
        {
            let mut config = state.config.write().map_err(|e| e.to_string())?;
            if provider == "openai" {
                config.openai_api_key = Some(key.clone());
            }
            config
                .save(&config.config_path())
                .map_err(|e| e.to_string())?;
        }

        // Rebuild router using centralized logic (preserves Superego)
        rebuild_router_with_superego(&state)?;
    }

    Ok(StoreKeyResult {
        success: true,
        provider,
        validated: should_validate,
        error: None,
    })
}

/// Get list of providers that have stored API keys.
#[tauri::command]
fn get_stored_providers(state: tauri::State<AppState>) -> Result<Vec<String>, String> {
    let vault = state.secrets.lock().map_err(|e| e.to_string())?;
    Ok(vault
        .list_providers()
        .iter()
        .map(|s| s.to_string())
        .collect())
}

/// Advance from Connectivity to Genesis.
#[tauri::command]
fn advance_to_genesis(state: tauri::State<AppState>) -> Result<(), String> {
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;
    b.clear_conversation(); // Clear connectivity conversation
    b.advance_to_genesis().map_err(|e| e.to_string())?;
    Ok(())
}

/// Extract name, purpose, and personality from the Genesis conversation.
/// Sends the conversation to the local LLM with an extraction prompt,
/// parses the JSON response, and returns the values for the SoulPreview form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisIdentity {
    pub name: Option<String>,
    pub purpose: Option<String>,
    pub personality: Option<String>,
}

#[tauri::command]
async fn extract_genesis_identity(
    state: tauri::State<'_, AppState>,
) -> Result<GenesisIdentity, String> {
    // Get conversation history from birth orchestrator
    let conversation = {
        let birth = state.birth.read().map_err(|e| e.to_string())?;
        let b = birth.as_ref().ok_or("Birth not started")?;
        b.get_conversation().to_vec()
    };

    if conversation.is_empty() {
        return Ok(GenesisIdentity {
            name: None,
            purpose: None,
            personality: None,
        });
    }

    // Build conversation transcript for the extraction prompt
    let mut conv_text = String::new();
    for (role, content) in &conversation {
        let label = match role.as_str() {
            "user" => "Mentor",
            "assistant" => "Abigail",
            _ => role.as_str(),
        };
        conv_text.push_str(&format!("{}: {}\n", label, content));
    }

    let extraction_prompt = format!(
        "Below is a conversation between a mentor and their AI agent during the agent's birth.\n\n\
         CONVERSATION:\n{}\n\n\
         Extract the following from the conversation and return ONLY a JSON object:\n\
         - \"name\": The name the mentor chose for the agent\n\
         - \"purpose\": What the agent's purpose should be\n\
         - \"personality\": The personality or tone the mentor described\n\n\
         If a value was not discussed, use null.\n\
         Return ONLY valid JSON, no other text. Example:\n\
         {{\"name\": \"Atlas\", \"purpose\": \"help with research\", \"personality\": \"witty and direct\"}}",
        conv_text
    );

    let messages = vec![abigail_capabilities::cognitive::Message::new(
        "user",
        &extraction_prompt,
    )];

    let router = state.router.read().map_err(|e| e.to_string())?.clone();
    let response = router.id_only(messages).await.map_err(|e| e.to_string())?;

    // Parse JSON from LLM response (best-effort)
    Ok(parse_identity_json(&response.content))
}

fn parse_identity_json(text: &str) -> GenesisIdentity {
    let empty = GenesisIdentity {
        name: None,
        purpose: None,
        personality: None,
    };

    // Try parsing the whole text as JSON
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text.trim()) {
        return value_to_identity(&v);
    }

    // Try to find a JSON object in the text
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            if end > start {
                let json_str = &text[start..=end];
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                    return value_to_identity(&v);
                }
            }
        }
    }

    empty
}

fn value_to_identity(v: &serde_json::Value) -> GenesisIdentity {
    GenesisIdentity {
        name: v.get("name").and_then(|v| v.as_str()).map(String::from),
        purpose: v.get("purpose").and_then(|v| v.as_str()).map(String::from),
        personality: v
            .get("personality")
            .and_then(|v| v.as_str())
            .map(String::from),
    }
}

/// Generate soul.md from template with name, purpose, personality.
/// Returns the generated content for preview.
#[tauri::command]
fn crystallize_soul(
    state: tauri::State<AppState>,
    name: String,
    purpose: String,
    personality: String,
) -> Result<String, String> {
    let soul_content = abigail_core::templates::fill_soul_template(&name, &purpose, &personality);
    let growth_content = abigail_core::templates::GROWTH_MD.to_string();

    // Update agent name in config
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.agent_name = Some(name.clone());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    // Write to disk and advance stage
    {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        let b = birth.as_mut().ok_or("Birth not started")?;
        b.crystallize_soul(&soul_content, &growth_content)
            .map_err(|e| e.to_string())?;
    }

    Ok(soul_content)
}

/// Sign all docs, finalize birth, write Trinity config.
#[tauri::command]
fn complete_emergence(state: tauri::State<AppState>) -> Result<(), String> {
    // Build Trinity config from current state
    let trinity = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let vault = state.secrets.lock().map_err(|e| e.to_string())?;

        TrinityConfig {
            id_url: config.local_llm_base_url.clone(),
            ego_provider: if config.openai_api_key.is_some() {
                Some("openai".to_string())
            } else {
                vault
                    .get_secret("anthropic")
                    .map(|_| "anthropic".to_string())
                    .or_else(|| vault.get_secret("xai").map(|_| "xai".to_string()))
            },
            ego_api_key: config
                .openai_api_key
                .clone()
                .or_else(|| vault.get_secret("anthropic").map(|s| s.to_string()))
                .or_else(|| vault.get_secret("xai").map(|s| s.to_string())),
            superego_provider: vault
                .get_secret("anthropic")
                .map(|_| "anthropic".to_string())
                .or_else(|| vault.get_secret("openai").map(|_| "openai".to_string())),
            superego_api_key: vault
                .get_secret("anthropic")
                .map(|s| s.to_string())
                .or_else(|| vault.get_secret("openai").map(|s| s.to_string())),
        }
    };

    // Complete emergence (sign docs, write birth memory)
    {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        let b = birth.as_mut().ok_or("Birth not started")?;
        b.complete_emergence().map_err(|e| e.to_string())?;
    }

    // Write operational (non-constitutional) documents to docs_dir
    {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let docs_dir = &config.docs_dir;
        let _ = std::fs::create_dir_all(docs_dir);
        let _ = std::fs::write(
            docs_dir.join("capabilities.md"),
            abigail_core::templates::CAPABILITIES_MD,
        );
        let _ = std::fs::write(
            docs_dir.join("triangle_ethics_operational.md"),
            abigail_core::templates::TRIANGLE_ETHICS_OPERATIONAL_MD,
        );
        tracing::info!("Wrote operational documents (capabilities.md, triangle_ethics_operational.md) to {:?}", docs_dir);
    }

    // Write trinity config and mark birth complete with timestamp
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.trinity = Some(trinity);
        config.birth_complete = true;
        config.birth_timestamp = Some(Utc::now().to_rfc3339());
        config.clear_birth_stage();
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    // Sync agent name to IdentityManager's global config
    {
        let active_agent = state
            .active_agent_id
            .read()
            .map_err(|e| e.to_string())?
            .clone();
        if let Some(agent_id) = active_agent {
            let config = state.config.read().map_err(|e| e.to_string())?;
            if let Some(name) = &config.agent_name {
                let _ = state.identity_manager.update_agent_name(&agent_id, name);
            }
        }
    }

    // Rebuild router so Ego + Superego are active immediately after birth
    rebuild_router_with_superego(&state)?;

    // Store encrypted birth transcript in per-agent Documents folder
    {
        let active_agent = state
            .active_agent_id
            .read()
            .map_err(|e| e.to_string())?
            .clone();
        if let Some(agent_id) = active_agent {
            let birth = state.birth.read().map_err(|e| e.to_string())?;
            if let Some(b) = birth.as_ref() {
                let conversation = b.get_conversation();
                if !conversation.is_empty() {
                    // Serialize and redact
                    let redacted: Vec<(String, String)> = conversation
                        .iter()
                        .map(|(role, content)| (role.clone(), redact_api_keys(content)))
                        .collect();
                    if let Ok(json) = serde_json::to_string_pretty(&redacted) {
                        if let Ok(docs_folder) =
                            state.identity_manager.create_documents_folder(&agent_id)
                        {
                            let transcript_path = docs_folder.join("birth_transcript.enc");
                            if let Err(e) = abigail_core::encrypted_storage::write_encrypted(
                                &transcript_path,
                                json.as_bytes(),
                            ) {
                                tracing::warn!("Failed to write birth transcript: {}", e);
                            } else {
                                tracing::info!(
                                    "Birth transcript saved: {}",
                                    transcript_path.display()
                                );
                            }

                            // Also store encrypted copies of constitutional documents
                            let config = state.config.read().map_err(|e| e.to_string())?;
                            for doc_name in &["soul.md", "ethics.md", "instincts.md"] {
                                let src = config.docs_dir.join(doc_name);
                                if src.exists() {
                                    if let Ok(content) = std::fs::read(&src) {
                                        let enc_name =
                                            format!("{}.enc", doc_name.trim_end_matches(".md"));
                                        let enc_path = docs_folder.join(enc_name);
                                        if let Err(e) =
                                            abigail_core::encrypted_storage::write_encrypted(
                                                &enc_path, &content,
                                            )
                                        {
                                            tracing::warn!("Failed to encrypt {}: {}", doc_name, e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Sign the active agent's public key with the Hive master key.
/// Called from BootSequence after `complete_emergence` to link the
/// birth-generated keypair into the Hive trust chain.
#[tauri::command]
fn sign_agent_with_hive(state: tauri::State<AppState>) -> Result<(), String> {
    let agent_id = state
        .active_agent_id
        .read()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or("No active agent")?;

    state.identity_manager.sign_agent_after_birth(&agent_id)
}

/// Read and decrypt the birth transcript for a given agent.
#[tauri::command]
fn get_birth_transcript(state: tauri::State<AppState>, agent_id: String) -> Result<String, String> {
    let docs_folder = state.identity_manager.create_documents_folder(&agent_id)?;
    let transcript_path = docs_folder.join("birth_transcript.enc");

    if !transcript_path.exists() {
        return Err("No birth transcript found for this agent".into());
    }

    let data = abigail_core::encrypted_storage::read_encrypted(&transcript_path)
        .map_err(|e| format!("Failed to decrypt transcript: {}", e))?;

    String::from_utf8(data).map_err(|e| format!("Invalid transcript encoding: {}", e))
}

// ── Subagent Management ──────────────────────────────────────────────

/// Serializable subagent info for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<String>,
}

/// List all registered subagent definitions.
#[tauri::command]
fn list_subagents(state: tauri::State<AppState>) -> Result<Vec<SubagentInfo>, String> {
    let mgr = state.subagent_manager.read().map_err(|e| e.to_string())?;
    Ok(mgr
        .list()
        .iter()
        .map(|d| SubagentInfo {
            id: d.id.clone(),
            name: d.name.clone(),
            description: d.description.clone(),
            capabilities: d.capabilities.clone(),
        })
        .collect())
}

/// Delegate a task to a specific subagent by id.
#[tauri::command]
async fn delegate_to_subagent(
    state: tauri::State<'_, AppState>,
    id: String,
    message: String,
) -> Result<String, String> {
    // Extract the router and subagent definition before the async boundary
    // to avoid holding the RwLock across await (RwLockReadGuard is !Send).
    let (router, def) = {
        let mgr = state.subagent_manager.read().map_err(|e| e.to_string())?;
        let def = mgr
            .list()
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .ok_or_else(|| format!("Subagent '{}' not found", id))?;
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (router, def)
    };

    let messages = vec![abigail_capabilities::cognitive::Message::new("user", &message)];

    // Delegate using the extracted router based on the subagent's provider
    let response = match &def.provider {
        abigail_router::SubagentProvider::SameAsEgo => {
            router
                .route_with_tools(messages, vec![])
                .await
                .map_err(|e| e.to_string())?
        }
        abigail_router::SubagentProvider::SameAsId => {
            router.id_only(messages).await.map_err(|e| e.to_string())?
        }
        abigail_router::SubagentProvider::Custom(_, _) => {
            // Custom providers not yet implemented — fall back to Ego route
            router
                .route_with_tools(messages, vec![])
                .await
                .map_err(|e| e.to_string())?
        }
    };

    Ok(response.content)
}

/// Determine the best Ego provider and API key from config + secrets vault.
/// Returns (provider_name, api_key). Checks TrinityConfig first, then
/// falls back to openai_api_key in config, then vault secrets.
fn determine_ego_provider(
    config: &AppConfig,
    secrets: &SecretsVault,
) -> (Option<String>, Option<String>) {
    // 1. TrinityConfig takes priority (set after birth)
    if let Some(ref trinity) = config.trinity {
        if trinity.ego_api_key.is_some() {
            tracing::info!(
                "determine_ego_provider: using TrinityConfig ego_provider={:?}",
                trinity.ego_provider
            );
            return (trinity.ego_provider.clone(), trinity.ego_api_key.clone());
        }
    }

    // 2. Legacy openai_api_key in config
    if let Some(ref key) = config.openai_api_key {
        if !key.is_empty() {
            tracing::info!("determine_ego_provider: using legacy openai_api_key from config");
            return (Some("openai".to_string()), Some(key.clone()));
        }
    }

    // 3. Check secrets vault for provider keys (all supported Ego providers)
    for provider in &["anthropic", "openai", "xai", "perplexity", "google"] {
        if let Some(key) = secrets.get_secret(provider) {
            tracing::info!(
                "determine_ego_provider: found key in vault for '{}'",
                provider
            );
            return (Some(provider.to_string()), Some(key.to_string()));
        }
    }

    tracing::warn!("determine_ego_provider: no Ego provider configured (no TrinityConfig, no API key, no vault secrets)");
    (None, None)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = get_config();

    // Initialize the Hive Identity Manager
    let identity_manager = Arc::new(
        IdentityManager::new(config.data_dir.clone()).unwrap_or_else(|e| {
            tracing::error!(
                "Failed to initialize IdentityManager: {}. Proceeding with default.",
                e
            );
            // Fall back to creating a basic manager
            IdentityManager::new(config.data_dir.clone())
                .expect("IdentityManager initialization failed fatally")
        }),
    );

    // Initialize the per-agent secrets vault (DPAPI-encrypted on Windows)
    let secrets = Arc::new(Mutex::new(
        SecretsVault::load(config.data_dir.clone())
            .unwrap_or_else(|_| SecretsVault::new(config.data_dir.clone())),
    ));

    // Initialize the hive-level secrets vault (shared API keys)
    let hive_secrets_dir = identity_manager.data_root().to_path_buf();
    let hive_secrets = Arc::new(Mutex::new(
        SecretsVault::load(hive_secrets_dir.clone())
            .unwrap_or_else(|_| SecretsVault::new(hive_secrets_dir)),
    ));

    // Determine Ego provider from config + vault + hive vault before building router
    let (ego_provider, ego_api_key) = {
        let vault = secrets.lock().unwrap();
        let (provider, key) = determine_ego_provider(&config, &vault);
        if provider.is_some() {
            (provider, key)
        } else {
            // Fall back to hive-level vault
            let hive = hive_secrets.lock().unwrap();
            determine_ego_provider(&config, &hive)
        }
    };

    tracing::info!(
        "Startup: ego_provider={:?}, local_url={:?}, mode={:?}",
        ego_provider,
        config.local_llm_base_url,
        config.routing_mode
    );
    if config.local_llm_base_url.is_some() {
        tracing::info!(
            "Startup: local LLM model name will be auto-detected on first set_local_llm_url call. \
             Until then, using default name 'local-model'."
        );
    }

    let router = {
        let mut r = IdEgoRouter::new(
            config.local_llm_base_url.clone(),
            ego_provider.as_deref(),
            ego_api_key,
            config.routing_mode,
        );

        // Attach Superego if configured in TrinityConfig (uses shared helper)
        if let Some((se_provider, se_key)) = extract_superego_config(&config) {
            let superego = build_superego_llm_provider(&se_provider, &se_key);
            r = r.with_superego(superego);
            tracing::info!("Superego configured at startup: provider={}", se_provider);
        }

        r
    };

    #[cfg(not(windows))]
    tracing::warn!(
        "Secrets are stored in plaintext on this platform (DPAPI is Windows-only). \
         Do not use for production; suitable for development only."
    );

    let registry = Arc::new(SkillRegistry::with_secrets(secrets.clone()));

    // Register built-in skills
    {
        let ws_manifest = WebSearchSkill::default_manifest();
        let ws = WebSearchSkill::with_secrets(ws_manifest.clone(), secrets.clone());
        let _ = registry.register(ws_manifest.id.clone(), Arc::new(ws));
    }
    {
        let fs_manifest = FilesystemSkill::default_manifest();
        // Sandbox to the user's home directory and Abigail data directory
        let mut allowed_roots = vec![config.data_dir.clone()];
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            allowed_roots.push(PathBuf::from(home));
        }
        let fs_skill = FilesystemSkill::new(fs_manifest.clone(), allowed_roots);
        let _ = registry.register(fs_manifest.id.clone(), Arc::new(fs_skill));
    }
    {
        let sh_manifest = ShellSkill::default_manifest();
        let sh_skill = ShellSkill::new(sh_manifest.clone());
        let _ = registry.register(sh_manifest.id.clone(), Arc::new(sh_skill));
    }
    {
        let http_manifest = HttpSkill::default_manifest();
        let http_skill = HttpSkill::new(http_manifest.clone());
        let _ = registry.register(http_manifest.id.clone(), Arc::new(http_skill));
    }
    {
        let pplx_manifest = PerplexitySearchSkill::default_manifest();
        let pplx_skill =
            PerplexitySearchSkill::with_secrets(pplx_manifest.clone(), secrets.clone());
        let _ = registry.register(pplx_manifest.id.clone(), Arc::new(pplx_skill));
    }

    let event_bus = Arc::new(EventBus::new(256));
    let executor = Arc::new(SkillExecutor::new(registry.clone()));

    // Capture data_dir before config is moved into AppState
    let data_dir = config.data_dir.clone();

    let subagent_manager = SubagentManager::new(Arc::new(router.clone()));

    let state = AppState {
        config: RwLock::new(config),
        birth: RwLock::new(None),
        router: RwLock::new(router),
        registry,
        executor,
        event_bus: event_bus.clone(),
        secrets,
        hive_secrets,
        identity_manager,
        active_agent_id: RwLock::new(None),
        subagent_manager: RwLock::new(subagent_manager),
    };

    // Clone event_bus before setup since state isn't available inside setup callback
    let event_bus_for_setup = event_bus.clone();

    // Start the skills directory watcher for hot-reload
    let skills_dir = data_dir.join("skills");
    let _skills_watcher = match abigail_skills::SkillsWatcher::start(vec![skills_dir]) {
        Ok((watcher, mut rx)) => {
            // Spawn a thread to forward skill file events to the Tauri event system
            // The watcher must be kept alive for the duration of the app
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("runtime");
                rt.block_on(async move {
                    while let Ok(event) = rx.recv().await {
                        let (event_type, path) = match event {
                            abigail_skills::SkillFileEvent::Changed(p) => ("changed", p),
                            abigail_skills::SkillFileEvent::Removed(p) => ("removed", p),
                        };
                        tracing::info!("Skill file {}: {}", event_type, path.display());
                        // Note: actual re-registration of skills would go here.
                        // For now we just log; full hot-reload requires dynamic loading.
                    }
                });
            });
            Some(watcher)
        }
        Err(e) => {
            tracing::warn!("Failed to start skills watcher: {}", e);
            None
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let event_bus = event_bus_for_setup.clone();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("runtime");
                rt.block_on(async move {
                    let mut rx = event_bus.subscribe();
                    while let Ok(ev) = rx.recv().await {
                        let payload = serde_json::json!({
                            "skill_id": ev.skill_id.0,
                            "trigger": ev.trigger,
                            "payload": ev.payload,
                            "timestamp": ev.timestamp.to_rfc3339(),
                            "priority": ev.priority as u8,
                        });
                        let _ = handle.emit("skill-event", payload);
                    }
                });
            });
            Ok(())
        })
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            // Hive identity management
            check_hive_status,
            get_identities,
            get_active_agent,
            load_agent,
            create_agent,
            disconnect_agent,
            migrate_legacy_identity,
            // Existing commands
            get_birth_complete,
            get_agent_name,
            get_docs_path,
            init_soul,
            generate_and_sign_constitutional,
            check_identity_status,
            check_existing_identity,
            archive_identity,
            wipe_identity,
            // SQLite management
            get_sqlite_stats,
            optimize_sqlite,
            backup_sqlite,
            reset_memories,
            check_interrupted_birth,
            repair_identity,
            run_startup_checks,
            get_birth_stage,
            get_birth_message,
            start_birth,
            verify_crypto,
            generate_identity,
            advance_past_darkness,
            configure_email,
            download_model,
            set_api_key,
            set_local_llm_url,
            get_router_status,
            set_superego_provider,
            complete_birth,
            skip_to_life_for_mvp,
            list_skills,
            list_discovered_skills,
            list_tools,
            execute_tool,
            get_mcp_servers,
            mcp_list_tools,
            execute_mcp_tool,
            get_mcp_app_content,
            list_approved_skills,
            install_skill,
            uninstall_skill,
            approve_skill,
            check_secret,
            store_secret,
            remove_secret,
            list_missing_skill_secrets,
            chat,
            chat_stream,
            // New birth flow commands
            probe_local_llm,
            set_local_llm_during_birth,
            birth_chat,
            store_provider_key,
            get_stored_providers,
            advance_to_genesis,
            extract_genesis_identity,
            crystallize_soul,
            complete_emergence,
            sign_agent_with_hive,
            get_birth_transcript,
            // Subagent management
            list_subagents,
            delegate_to_subagent,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
