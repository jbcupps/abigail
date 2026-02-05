#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod templates;

use ao_birth::BirthOrchestrator;
use ao_core::{
    generate_external_keypair, sign_constitutional_documents, AppConfig,
    CoreError, ExternalVault, Keyring, ReadOnlyFileVault, SecretsVault, TrinityConfig, Verifier,
};
use ao_memory::{Memory, MemoryStore};
use ao_router::IdEgoRouter;
use ao_skills::channel::EventBus;
use ao_skills::{MissingSkillSecret, SkillExecutor, SkillRegistry, ToolParams};
use base64::Engine as _;
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use tauri::Emitter;

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

    // Environment variable fallbacks
    if config.local_llm_base_url.is_none() {
        config.local_llm_base_url = std::env::var("LOCAL_LLM_BASE_URL")
            .ok()
            .filter(|s| !s.is_empty());
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
fn generate_and_sign_constitutional(state: tauri::State<AppState>) -> Result<KeypairGenerationResult, String> {
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
        return Err(
            "Constitutional documents are already signed. \
             The private key was presented during initial setup and is not stored by AO. \
             If you need to re-sign, you must use your saved private key."
                .to_string(),
        );
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
        config.save(&config.config_path()).map_err(|e| e.to_string())?;
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
async fn run_startup_checks(state: tauri::State<'_, AppState>) -> Result<StartupCheckResult, String> {
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

    tracing::info!("Verifying crypto integrity in: {}", docs_path.display());

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

    let private_key = b.get_private_key_base64()
        .ok_or("No private key generated")?
        .to_string();

    let data_dir = b.config().data_dir.clone();
    let pubkey_path = data_dir.join("external_pubkey.bin");

    // Also sync config to AppState
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.external_pubkey_path = Some(pubkey_path.clone());
        config.save(&config.config_path()).map_err(|e| e.to_string())?;
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
    b.advance_past_darkness();
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairIdentityParams {
    pub private_key: Option<String>,
    pub reset: bool,
}

#[tauri::command]
fn repair_identity(state: tauri::State<AppState>, params: RepairIdentityParams) -> Result<(), String> {
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
            return Err("Public key not found. Cannot verify ownership. Please use Reset.".to_string());
        }

        let vault = ReadOnlyFileVault::new(&pubkey_path);
        let stored_pubkey = vault.read_public_key().map_err(|e: CoreError| e.to_string())?;

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
    let password_encrypted = Keyring::encrypt_bytes(password.as_bytes())
        .map_err(|e| e.to_string())?;
    config.email = Some(ao_core::EmailConfig {
        address,
        imap_host,
        imap_port,
        smtp_host,
        smtp_port,
        password_encrypted,
    });
    config.save(&config.config_path()).map_err(|e| e.to_string())?;
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
    config.save(&config.config_path()).map_err(|e| e.to_string())?;

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
fn set_local_llm_url(state: tauri::State<AppState>, url: String) -> Result<(), String> {
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    config.local_llm_base_url = if url.is_empty() { None } else { Some(url) };
    config.save(&config.config_path()).map_err(|e| e.to_string())?;

    // Rebuild the router so it picks up the new URL
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

/// Status of the Id/Ego router for debugging and UI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterStatus {
    /// Id provider type: "candle_stub", "local_http", or "ollama"
    pub id_provider: String,
    /// Local LLM URL if configured
    pub id_url: Option<String>,
    /// Whether Ego (OpenAI) is configured
    pub ego_configured: bool,
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
        ego_configured: config.openai_api_key.is_some(),
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
    config.save(&config.config_path()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn list_skills(state: tauri::State<AppState>) -> Result<Vec<ao_skills::SkillManifest>, String> {
    state
        .registry
        .list()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_discovered_skills(state: tauri::State<AppState>) -> Result<Vec<ao_skills::SkillManifest>, String> {
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
    let tool_params = ToolParams {
        values: params,
    };
    state
        .executor
        .execute(&id, &tool_name, tool_params)
        .await
        .map_err(|e| e.to_string())
}

// ── Secrets Management ──────────────────────────────────────────────

#[tauri::command]
fn check_secret(state: tauri::State<AppState>, key: String) -> Result<bool, String> {
    let vault = state.secrets.lock().map_err(|e| e.to_string())?;
    Ok(vault.exists(&key))
}

#[tauri::command]
fn store_secret(state: tauri::State<AppState>, key: String, value: String) -> Result<(), String> {
    let mut vault = state.secrets.lock().map_err(|e| e.to_string())?;
    vault.set_secret(&key, &value);
    vault.save().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn remove_secret(state: tauri::State<AppState>, key: String) -> Result<bool, String> {
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

#[tauri::command]
async fn chat(state: tauri::State<'_, AppState>, message: String) -> Result<String, String> {
    // Get store and clone router before await (RwLock is not Send)
    let (store, router) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let store = MemoryStore::open_with_config(&*config).map_err(|e| e.to_string())?;
        drop(config);
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (store, router)
    };
    let messages = vec![ao_capabilities::cognitive::Message {
        role: "user".to_string(),
        content: message.clone(),
    }];
    let response = router
        .route(messages.clone())
        .await
        .map_err(|e| e.to_string())?;
    let memory = Memory::ephemeral(format!("user: {} | assistant: {}", message, response.content));
    let _ = store.insert_memory(&memory);
    Ok(response.content)
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
    // Set the URL in config and rebuild router
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.local_llm_base_url = Some(url.clone());
        config.save(&config.config_path()).map_err(|e| e.to_string())?;

        let new_router = IdEgoRouter::new(
            config.local_llm_base_url.clone(),
            config.openai_api_key.clone(),
            config.routing_mode,
        );
        drop(config);
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
            b.config_mut().local_llm_base_url = Some(url);
            b.advance_to_connectivity();
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
#[tauri::command]
async fn birth_chat(
    state: tauri::State<'_, AppState>,
    message: String,
) -> Result<BirthChatResponse, String> {
    // Get stage and system prompt
    let (stage, system_prompt) = {
        let birth = state.birth.read().map_err(|e| e.to_string())?;
        let b = birth.as_ref().ok_or("Birth not started")?;
        let stage = b.current_stage();
        let prompt = ao_birth::prompts::system_prompt_for_stage(stage)
            .unwrap_or("You are AO, a newborn AI agent.");
        (stage, prompt.to_string())
    };

    // Record user message
    {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        let b = birth.as_mut().ok_or("Birth not started")?;
        b.add_message("user", &message);
    }

    // Build messages array with system prompt + conversation history
    let messages = {
        let birth = state.birth.read().map_err(|e| e.to_string())?;
        let b = birth.as_ref().ok_or("Birth not started")?;
        let mut msgs = vec![ao_capabilities::cognitive::Message {
            role: "system".to_string(),
            content: system_prompt,
        }];
        for (role, content) in b.get_conversation() {
            msgs.push(ao_capabilities::cognitive::Message {
                role: role.clone(),
                content: content.clone(),
            });
        }
        msgs
    };

    // Route through local LLM only (Id)
    let router = state.router.read().map_err(|e| e.to_string())?.clone();
    let response = router.id_only(messages).await.map_err(|e| e.to_string())?;

    // Record assistant response
    {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        let b = birth.as_mut().ok_or("Birth not started")?;
        b.add_message("assistant", &response.content);
    }

    Ok(BirthChatResponse {
        message: response.content,
        stage: stage.name().to_string(),
        action: None,
    })
}

/// Store a provider API key in the vault during Connectivity.
#[tauri::command]
fn store_provider_key(
    state: tauri::State<AppState>,
    provider: String,
    key: String,
) -> Result<bool, String> {
    // Store in secrets vault
    {
        let mut vault = state.secrets.lock().map_err(|e| e.to_string())?;
        vault.set_secret(&provider, &key);
        vault.save().map_err(|e| e.to_string())?;
    }

    // If it's an OpenAI key, also set it as the main API key and rebuild router
    if provider == "openai" {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.openai_api_key = Some(key.clone());
        config.save(&config.config_path()).map_err(|e| e.to_string())?;

        let new_router = IdEgoRouter::new(
            config.local_llm_base_url.clone(),
            config.openai_api_key.clone(),
            config.routing_mode,
        );
        drop(config);
        let mut router = state.router.write().map_err(|e| e.to_string())?;
        *router = new_router;
    }

    Ok(true)
}

/// Advance from Connectivity to Genesis.
#[tauri::command]
fn advance_to_genesis(state: tauri::State<AppState>) -> Result<(), String> {
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;
    b.clear_conversation(); // Clear connectivity conversation
    b.advance_to_genesis();
    Ok(())
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
        config.save(&config.config_path()).map_err(|e| e.to_string())?;
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
                vault.get_secret("anthropic").map(|_| "anthropic".to_string())
                    .or_else(|| vault.get_secret("xai").map(|_| "xai".to_string()))
            },
            ego_api_key: config.openai_api_key.clone()
                .or_else(|| vault.get_secret("anthropic").map(|s| s.to_string()))
                .or_else(|| vault.get_secret("xai").map(|s| s.to_string())),
            superego_provider: None, // Follow-up: 3-way routing
            superego_api_key: None,
        }
    };

    // Complete emergence (sign docs, write birth memory)
    {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        let b = birth.as_mut().ok_or("Birth not started")?;
        b.complete_emergence().map_err(|e| e.to_string())?;
    }

    // Write trinity config
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.trinity = Some(trinity);
        config.birth_complete = true;
        config.save(&config.config_path()).map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = get_config();
    let router = IdEgoRouter::new(
        config.local_llm_base_url.clone(),
        config.openai_api_key.clone(),
        config.routing_mode,
    );

    // Initialize the secrets vault (DPAPI-encrypted on Windows)
    let secrets = Arc::new(Mutex::new(
        SecretsVault::load(config.data_dir.clone())
            .unwrap_or_else(|_| SecretsVault::new(config.data_dir.clone())),
    ));

    let registry = Arc::new(SkillRegistry::with_secrets(secrets.clone()));
    let event_bus = Arc::new(EventBus::new(256));
    let executor = Arc::new(SkillExecutor::new(registry.clone()));

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

    tauri::Builder::default()
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
            get_docs_path,
            init_soul,
            generate_and_sign_constitutional,
            check_identity_status,
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
            // New birth flow commands
            probe_local_llm,
            set_local_llm_during_birth,
            birth_chat,
            store_provider_key,
            advance_to_genesis,
            crystallize_soul,
            complete_emergence,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
