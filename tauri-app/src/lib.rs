#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod templates;

use ao_birth::BirthOrchestrator;
use ao_core::{
    generate_external_keypair, sign_constitutional_documents, validate_local_llm_url, AppConfig,
    CoreError, ExternalVault, Keyring, ReadOnlyFileVault, SecretsVault, TrinityConfig, Verifier,
};
use ao_memory::{Memory, MemoryStore};
use ao_router::IdEgoRouter;
use ao_skills::channel::EventBus;
use ao_skills::{MissingSkillSecret, SkillExecutor, SkillRegistry, ToolParams};
use base64::Engine as _;
use chrono::Utc;
use ed25519_dalek::SigningKey;
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

struct AppState {
    config: RwLock<AppConfig>,
    birth: RwLock<Option<BirthOrchestrator>>,
    router: RwLock<IdEgoRouter>,
    registry: Arc<SkillRegistry>,
    executor: Arc<SkillExecutor>,
    #[allow(dead_code)] // used for skill-event subscription; keep for future UI wiring
    event_bus: Arc<EventBus>,
    secrets: Arc<Mutex<SecretsVault>>,
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
/// CRITICAL: The private key is returned ONCE and never stored by AO.
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
             The private key was presented during initial setup and is not stored by AO. \
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
        "ao_seed.db",
        "ao_seed.db-wal",
        "ao_seed.db-shm",
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
        *vault = ao_core::SecretsVault::new(data_dir.clone());
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
        "ao_seed.db",
        "ao_seed.db-wal",
        "ao_seed.db-shm",
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
        *vault = ao_core::SecretsVault::new(data_dir);
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
    config.email = Some(ao_core::EmailConfig {
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
    let downloader = ao_capabilities::cognitive::ModelDownloader::new();
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
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    config.openai_api_key = Some(key);
    config
        .save(&config.config_path())
        .map_err(|e| e.to_string())?;

    // Rebuild the router so it picks up the new API key
    let new_router = IdEgoRouter::new(
        config.local_llm_base_url.clone(),
        config.openai_api_key.clone(),
        config.routing_mode,
    );
    drop(config); // Release config lock before acquiring router lock
    let mut router = state.router.write().map_err(|e| e.to_string())?;
    *router = new_router;
    Ok(())
}

#[tauri::command]
async fn set_local_llm_url(state: tauri::State<'_, AppState>, url: String) -> Result<(), String> {
    let (local_url, api_key, mode) = {
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
        (
            config.local_llm_base_url.clone(),
            config.openai_api_key.clone(),
            config.routing_mode,
        )
    };

    // Rebuild the router with auto-detected model name (important for LM Studio)
    let new_router = IdEgoRouter::new_auto_detect(local_url, api_key, mode).await;
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

/// Rebuild the router from current config + vault state, attaching Superego if configured.
fn rebuild_router_with_superego(state: &AppState) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let vault = state.secrets.lock().map_err(|e| e.to_string())?;

    // Determine ego provider (priority: config key > vault keys by preference)
    let (ego_name, ego_key) = if let Some(ref key) = config.openai_api_key {
        (Some("openai"), Some(key.clone()))
    } else if let Some(key) = vault.get_secret("anthropic") {
        (Some("anthropic"), Some(key.to_string()))
    } else if let Some(key) = vault.get_secret("openai") {
        (Some("openai"), Some(key.to_string()))
    } else if let Some(key) = vault.get_secret("xai") {
        (Some("xai"), Some(key.to_string()))
    } else if let Some(key) = vault.get_secret("perplexity") {
        (Some("perplexity"), Some(key.to_string()))
    } else if let Some(key) = vault.get_secret("google") {
        (Some("google"), Some(key.to_string()))
    } else {
        (None, None)
    };

    let mut new_router = IdEgoRouter::with_provider(
        config.local_llm_base_url.clone(),
        ego_name,
        ego_key,
        config.routing_mode,
    );

    // Attach Superego if configured in TrinityConfig
    if let Some(ref trinity) = config.trinity {
        if let (Some(ref se_provider), Some(ref se_key)) =
            (&trinity.superego_provider, &trinity.superego_api_key)
        {
            if !se_key.is_empty() {
                let superego: std::sync::Arc<dyn ao_capabilities::cognitive::LlmProvider> =
                    match se_provider.as_str() {
                        "anthropic" => std::sync::Arc::new(
                            ao_capabilities::cognitive::AnthropicProvider::new(se_key.clone()),
                        ),
                        "perplexity" | "xai" | "google" => {
                            if let Some(cp) =
                                ao_capabilities::cognitive::CompatibleProvider::from_name(
                                    se_provider,
                                )
                            {
                                std::sync::Arc::new(
                                    ao_capabilities::cognitive::OpenAiCompatibleProvider::new(
                                        cp,
                                        se_key.clone(),
                                    ),
                                )
                            } else {
                                std::sync::Arc::new(
                                    ao_capabilities::cognitive::OpenAiProvider::new(Some(
                                        se_key.clone(),
                                    )),
                                )
                            }
                        }
                        _ => std::sync::Arc::new(ao_capabilities::cognitive::OpenAiProvider::new(
                            Some(se_key.clone()),
                        )),
                    };
                new_router = new_router.with_superego(superego);
                tracing::info!("Superego attached: provider={}", se_provider);
            }
        }
    }

    drop(vault);
    drop(config);
    let mut router = state.router.write().map_err(|e| e.to_string())?;
    *router = new_router;
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
    let router = state.router.read().map_err(|e| e.to_string())?;

    let id_provider = if router.is_using_http_provider() {
        "local_http".to_string()
    } else {
        "candle_stub".to_string()
    };

    Ok(RouterStatus {
        id_provider,
        id_url: config.local_llm_base_url.clone(),
        ego_configured: router.has_ego(),
        ego_provider: router.ego_provider_name().map(|p| p.to_string()),
        superego_configured: router.has_superego(),
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
fn list_skills(state: tauri::State<AppState>) -> Result<Vec<ao_skills::SkillManifest>, String> {
    state.registry.list().map_err(|e| e.to_string())
}

#[tauri::command]
fn list_discovered_skills(
    state: tauri::State<AppState>,
) -> Result<Vec<ao_skills::SkillManifest>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let paths = vec![config.data_dir.join("skills")];
    Ok(ao_skills::SkillRegistry::discover(&paths))
}

#[tauri::command]
fn list_tools(
    state: tauri::State<AppState>,
    skill_id: String,
) -> Result<Vec<ao_skills::ToolDescriptor>, String> {
    let id = ao_skills::SkillId(skill_id);
    let (skill, _) = state.registry.get_skill(&id).map_err(|e| e.to_string())?;
    Ok(skill.tools())
}

#[tauri::command]
async fn execute_tool(
    state: tauri::State<'_, AppState>,
    skill_id: String,
    tool_name: String,
    params: HashMap<String, serde_json::Value>,
) -> Result<ao_skills::ToolOutput, String> {
    let id = ao_skills::SkillId(skill_id);
    let tool_params = ToolParams { values: params };
    state
        .executor
        .execute(&id, &tool_name, tool_params)
        .await
        .map_err(|e| e.to_string())
}

// ── Secrets Management ──────────────────────────────────────────────

/// Reserved provider names for API keys (must match validation.rs known providers).
const ALLOWED_PROVIDER_SECRET_KEYS: &[&str] = &["openai", "anthropic", "xai", "google", "tavily"];

/// Returns the set of allowed secret keys: reserved provider names + skill-declared secret names.
fn allowed_secret_keys(
    registry: &SkillRegistry,
    skill_paths: &[PathBuf],
) -> std::collections::HashSet<String> {
    let mut allowed: std::collections::HashSet<String> = ALLOWED_PROVIDER_SECRET_KEYS
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let manifests = ao_skills::SkillRegistry::discover(skill_paths);
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
) -> Vec<ao_capabilities::cognitive::ToolDefinition> {
    let mut tools = Vec::new();

    // Built-in: store_provider_key
    let schema = ao_capabilities::cognitive::update_provider_key_schema();
    tools.push(ao_capabilities::cognitive::ToolDefinition {
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
                    tools.push(ao_capabilities::cognitive::ToolDefinition {
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

/// Execute a tool call from the LLM and return the result string.
async fn execute_tool_call(
    state: &tauri::State<'_, AppState>,
    app: &tauri::AppHandle,
    tool_call: &ao_capabilities::cognitive::ToolCall,
) -> String {
    match tool_call.name.as_str() {
        "update_provider_key" | "store_provider_key" => {
            let args: serde_json::Value = match serde_json::from_str(&tool_call.arguments) {
                Ok(v) => v,
                Err(e) => return format!("Error parsing arguments: {}", e),
            };
            let provider = args["provider"].as_str().unwrap_or("");
            let key = args["key"].as_str().unwrap_or("");

            if provider.is_empty() || key.is_empty() {
                return "Error: provider and key are required".to_string();
            }

            // Validate the key first
            if let Err(e) =
                ao_capabilities::cognitive::validation::validate_api_key(provider, key).await
            {
                return format!(
                    "API key validation failed: {}. Please check the key and try again.",
                    e
                );
            }

            // Store in secrets vault
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

            // Rebuild router for cloud provider keys
            match provider {
                "openai" => {
                    if let Ok(mut config) = state.config.write() {
                        config.openai_api_key = Some(key.to_string());
                        let _ = config.save(&config.config_path());

                        let new_router = IdEgoRouter::with_provider(
                            config.local_llm_base_url.clone(),
                            Some("openai"),
                            Some(key.to_string()),
                            config.routing_mode,
                        );
                        drop(config);
                        if let Ok(mut router) = state.router.write() {
                            *router = new_router;
                        }
                    }
                }
                "anthropic" => {
                    if let Ok(config) = state.config.read() {
                        let new_router = IdEgoRouter::with_provider(
                            config.local_llm_base_url.clone(),
                            Some("anthropic"),
                            Some(key.to_string()),
                            config.routing_mode,
                        );
                        drop(config);
                        if let Ok(mut router) = state.router.write() {
                            *router = new_router;
                        }
                    }
                }
                _ => {}
            }

            format!(
                "Successfully validated and stored {} API key in secure vault.",
                provider
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
fn parse_text_tool_calls(content: &str) -> (Vec<ao_capabilities::cognitive::ToolCall>, String) {
    let mut tool_calls = Vec::new();
    let mut cleaned_content = content.to_string();

    // Helper to try parsing a JSON string as a tool call
    let try_parse_tool = |json_str: &str| -> Option<ao_capabilities::cognitive::ToolCall> {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
            if let Some(name) = parsed.get("name").and_then(|n| n.as_str()) {
                let arguments = if let Some(args) = parsed.get("arguments") {
                    args.to_string()
                } else {
                    "{}".to_string()
                };
                return Some(ao_capabilities::cognitive::ToolCall {
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
            ao_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        drop(config);
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (store, router, prompt)
    };

    // Build messages with system prompt
    let mut messages = vec![
        ao_capabilities::cognitive::Message::new("system", &system_prompt),
        ao_capabilities::cognitive::Message::new("user", &message),
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
            messages.push(ao_capabilities::cognitive::Message {
                role: "assistant".to_string(),
                content: response.content.clone(),
                tool_call_id: None,
                tool_calls: Some(tool_calls.clone()),
            });

            for (tc, result) in &tool_results {
                messages.push(ao_capabilities::cognitive::Message::tool_result(
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
            messages.push(ao_capabilities::cognitive::Message::new(
                "assistant",
                &cleaned_content,
            ));

            // Add tool results as a system message so the LLM knows what happened
            let results_summary: String = tool_results
                .iter()
                .map(|(name, result)| format!("[Tool '{}' result]: {}", name, result))
                .collect::<Vec<_>>()
                .join("\n");
            messages.push(ao_capabilities::cognitive::Message::new(
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
    use ao_capabilities::cognitive::StreamEvent;

    let (store, router, system_prompt) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let store = MemoryStore::open_with_config(&*config).map_err(|e| e.to_string())?;
        let prompt =
            ao_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        drop(config);
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (store, router, prompt)
    };

    let mut messages = vec![
        ao_capabilities::cognitive::Message::new("system", &system_prompt),
        ao_capabilities::cognitive::Message::new("user", &message),
    ];

    let target_mode = target.as_deref().unwrap_or("EGO");
    let tools = chat_tool_definitions(&state.registry);

    // For simple (no-tool) streaming, use the streaming path.
    // For tool-calling, we do a non-streaming initial request then stream the follow-up.
    let final_content = if target_mode == "ID" {
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

        let request = ao_capabilities::cognitive::CompletionRequest::simple(messages.clone());
        let response = router
            .route_stream(messages, tx)
            .await
            .map_err(|e| e.to_string())?;
        let _ = forward_handle.await;
        response.content
    } else {
        // Ego mode: first request with tools (non-streaming to capture tool calls)
        let response = router
            .route_with_tools(messages.clone(), tools.clone())
            .await
            .map_err(|e| e.to_string())?;

        if let Some(ref tool_calls) = response.tool_calls {
            // Execute tools (non-streaming)
            let mut tool_results = Vec::new();
            for tc in tool_calls {
                let result = execute_tool_call(&state, &app, tc).await;
                tool_results.push((tc.clone(), result));
            }

            messages.push(ao_capabilities::cognitive::Message {
                role: "assistant".to_string(),
                content: response.content.clone(),
                tool_call_id: None,
                tool_calls: Some(tool_calls.clone()),
            });

            for (tc, result) in &tool_results {
                messages.push(ao_capabilities::cognitive::Message::tool_result(
                    &tc.id, result,
                ));
            }

            // Stream the follow-up response
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

            let follow_up = router
                .route_stream_with_tools(messages, tools, tx)
                .await
                .map_err(|e| e.to_string())?;
            let _ = forward_handle.await;
            follow_up.content
        } else {
            // No tool calls — the initial response was already the final answer.
            // We didn't stream it, so emit it now as a single token.
            let _ = app.emit(
                "chat-token",
                serde_json::json!({ "token": response.content }),
            );
            let _ = app.emit("chat-token", serde_json::json!({ "done": true }));
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
    let (api_key, mode) = {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.local_llm_base_url = Some(normalized_url.clone());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
        (config.openai_api_key.clone(), config.routing_mode)
    };

    // Rebuild router with auto-detected model name (important for LM Studio)
    let new_router =
        IdEgoRouter::new_auto_detect(Some(normalized_url.clone()), api_key, mode).await;
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
            ao_birth::prompts::system_prompt_for_stage_with_context(stage, &stored_providers)
                .unwrap_or_else(|| "You are AO, a newborn AI agent.".to_string());
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
        let mut msgs = vec![ao_capabilities::cognitive::Message::new(
            "system",
            &system_prompt,
        )];
        for (role, content) in b.get_conversation() {
            msgs.push(ao_capabilities::cognitive::Message::new(role, content));
        }
        msgs
    };

    // Route through local LLM only (Id)
    let router = state.router.read().map_err(|e| e.to_string())?.clone();
    let response = router
        .id_only(messages.clone())
        .await
        .map_err(|e| e.to_string())?;

    // Check for text-based tool calls (for local LLMs without native function calling)
    let (text_tool_calls, cleaned_content) = parse_text_tool_calls(&response.content);

    let final_content = if !text_tool_calls.is_empty() {
        // Execute text-based tool calls
        let mut tool_results = Vec::new();
        for tc in &text_tool_calls {
            let result = execute_tool_call(&state, &app, tc).await;
            tool_results.push((tc.name.clone(), result));
        }

        // Build follow-up with tool results
        messages.push(ao_capabilities::cognitive::Message::new(
            "assistant",
            &cleaned_content,
        ));

        // Add tool results as a system message
        let results_summary: String = tool_results
            .iter()
            .map(|(name, result)| format!("[Tool '{}' executed]: {}", name, result))
            .collect::<Vec<_>>()
            .join("\n");
        messages.push(ao_capabilities::cognitive::Message::new(
            "system",
            &results_summary,
        ));

        // Get follow-up response from Id to acknowledge the tool execution
        let follow_up = router.id_only(messages).await.map_err(|e| e.to_string())?;

        // Record the full exchange in birth conversation
        {
            let mut birth = state.birth.write().map_err(|e| e.to_string())?;
            let b = birth.as_mut().ok_or("Birth not started")?;
            b.add_message("assistant", &cleaned_content);
            b.add_message("system", &results_summary);
            b.add_message("assistant", &follow_up.content);
        }

        follow_up.content
    } else {
        // No tool calls - record response normally
        {
            let mut birth = state.birth.write().map_err(|e| e.to_string())?;
            let b = birth.as_mut().ok_or("Birth not started")?;
            b.add_message("assistant", &response.content);
        }
        response.content
    };

    Ok(BirthChatResponse {
        message: final_content,
        stage: stage.name().to_string(),
        action: None,
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

    // Validate if requested
    if should_validate {
        if let Err(e) =
            ao_capabilities::cognitive::validation::validate_api_key(&provider, &key).await
        {
            return Ok(StoreKeyResult {
                success: false,
                provider,
                validated: false,
                error: Some(e.to_string()),
            });
        }
    }

    // Store in secrets vault
    {
        let mut vault = state.secrets.lock().map_err(|e| e.to_string())?;
        vault.set_secret(&provider, &key);
        vault.save().map_err(|e| e.to_string())?;
    }

    // Rebuild router for cloud provider keys
    match provider.as_str() {
        "openai" => {
            let mut config = state.config.write().map_err(|e| e.to_string())?;
            config.openai_api_key = Some(key.clone());
            config
                .save(&config.config_path())
                .map_err(|e| e.to_string())?;

            let new_router = IdEgoRouter::with_provider(
                config.local_llm_base_url.clone(),
                Some("openai"),
                Some(key.clone()),
                config.routing_mode,
            );
            drop(config);
            let mut router = state.router.write().map_err(|e| e.to_string())?;
            *router = new_router;
        }
        "anthropic" => {
            let config = state.config.read().map_err(|e| e.to_string())?;
            let new_router = IdEgoRouter::with_provider(
                config.local_llm_base_url.clone(),
                Some("anthropic"),
                Some(key.clone()),
                config.routing_mode,
            );
            drop(config);
            let mut router = state.router.write().map_err(|e| e.to_string())?;
            *router = new_router;
        }
        _ => {} // Other providers don't need router rebuild
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
            "assistant" => "AO",
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

    let messages = vec![ao_capabilities::cognitive::Message::new(
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
    let soul_content = ao_core::templates::fill_soul_template(&name, &purpose, &personality);
    let growth_content = ao_core::templates::GROWTH_MD.to_string();

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

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = get_config();

    // Initialize the secrets vault (DPAPI-encrypted on Windows)
    let secrets = Arc::new(Mutex::new(
        SecretsVault::load(config.data_dir.clone())
            .unwrap_or_else(|_| SecretsVault::new(config.data_dir.clone())),
    ));

    // Build router, selecting best available Ego provider from vault/config
    let router = {
        let vault = secrets.lock().unwrap();
        let mut r = if let Some(ref key) = config.openai_api_key {
            // Explicit OpenAI key in config takes precedence
            IdEgoRouter::with_provider(
                config.local_llm_base_url.clone(),
                Some("openai"),
                Some(key.clone()),
                config.routing_mode,
            )
        } else if let Some(key) = vault.get_secret("anthropic") {
            IdEgoRouter::with_provider(
                config.local_llm_base_url.clone(),
                Some("anthropic"),
                Some(key.to_string()),
                config.routing_mode,
            )
        } else if let Some(key) = vault.get_secret("openai") {
            IdEgoRouter::with_provider(
                config.local_llm_base_url.clone(),
                Some("openai"),
                Some(key.to_string()),
                config.routing_mode,
            )
        } else if let Some(key) = vault.get_secret("xai") {
            IdEgoRouter::with_provider(
                config.local_llm_base_url.clone(),
                Some("xai"),
                Some(key.to_string()),
                config.routing_mode,
            )
        } else if let Some(key) = vault.get_secret("perplexity") {
            IdEgoRouter::with_provider(
                config.local_llm_base_url.clone(),
                Some("perplexity"),
                Some(key.to_string()),
                config.routing_mode,
            )
        } else if let Some(key) = vault.get_secret("google") {
            IdEgoRouter::with_provider(
                config.local_llm_base_url.clone(),
                Some("google"),
                Some(key.to_string()),
                config.routing_mode,
            )
        } else {
            IdEgoRouter::new(config.local_llm_base_url.clone(), None, config.routing_mode)
        };

        // Attach Superego if configured in TrinityConfig
        if let Some(ref trinity) = config.trinity {
            if let (Some(ref se_provider), Some(ref se_key)) =
                (&trinity.superego_provider, &trinity.superego_api_key)
            {
                if !se_key.is_empty() {
                    let superego: Arc<dyn ao_capabilities::cognitive::LlmProvider> =
                        match se_provider.as_str() {
                            "anthropic" => Arc::new(
                                ao_capabilities::cognitive::AnthropicProvider::new(se_key.clone()),
                            ),
                            _ => Arc::new(ao_capabilities::cognitive::OpenAiProvider::new(Some(
                                se_key.clone(),
                            ))),
                        };
                    r = r.with_superego(superego);
                    tracing::info!("Superego configured at startup: provider={}", se_provider);
                }
            }
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
        // Sandbox to the user's home directory and AO data directory
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

    let state = AppState {
        config: RwLock::new(config),
        birth: RwLock::new(None),
        router: RwLock::new(router),
        registry,
        executor,
        event_bus: event_bus.clone(),
        secrets,
    };

    // Clone event_bus before setup since state isn't available inside setup callback
    let event_bus_for_setup = event_bus.clone();

    // Start the skills directory watcher for hot-reload
    let skills_dir = data_dir.join("skills");
    let _skills_watcher = match ao_skills::SkillsWatcher::start(vec![skills_dir]) {
        Ok((watcher, mut rx)) => {
            // Spawn a thread to forward skill file events to the Tauri event system
            // The watcher must be kept alive for the duration of the app
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("runtime");
                rt.block_on(async move {
                    while let Ok(event) = rx.recv().await {
                        let (event_type, path) = match event {
                            ao_skills::SkillFileEvent::Changed(p) => ("changed", p),
                            ao_skills::SkillFileEvent::Removed(p) => ("removed", p),
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
