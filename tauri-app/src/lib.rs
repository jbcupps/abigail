#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod templates;

use abby_birth::BirthOrchestrator;
use abby_core::document::{CoreDocument, DocumentTier};
use abby_core::{write_sig_file, AppConfig, Keyring};
use abby_memory::{Memory, MemoryStore};
use abby_router::IdEgoRouter;
use abby_skills::{SkillExecutor, SkillRegistry, ToolParams};
use abby_skills::channel::EventBus;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::Signer;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tauri::Emitter;

struct AppState {
    config: RwLock<AppConfig>,
    birth: RwLock<Option<BirthOrchestrator>>,
    router: IdEgoRouter,
    registry: Arc<SkillRegistry>,
    executor: Arc<SkillExecutor>,
    #[allow(dead_code)] // used for skill-event subscription; keep for future UI wiring
    event_bus: Arc<EventBus>,
}

fn get_config() -> AppConfig {
    let config = AppConfig::default_paths();
    let path = config.config_path();
    if path.exists() {
        AppConfig::load(&path).unwrap_or(config)
    } else {
        config
    }
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

/// One-time setup: generate keyring, write constitutional docs, sign them. Idempotent if keys exist.
#[tauri::command]
fn init_soul(state: tauri::State<AppState>) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let data_dir = config.data_dir.clone();
    let docs_dir = config.docs_dir.clone();
    let keys_file = data_dir.join("keys.bin");
    if keys_file.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(&docs_dir).map_err(|e| e.to_string())?;
    let (keyring, install_signing) = Keyring::generate(data_dir.clone()).map_err(|e| e.to_string())?;
    let docs = [
        ("soul.md", templates::SOUL_MD),
        ("ethics.md", templates::ETHICS_MD),
        ("instincts.md", templates::INSTINCTS_MD),
    ];
    for (name, content) in docs {
        let path = docs_dir.join(name);
        std::fs::write(&path, content).map_err(|e| e.to_string())?;
        let mut doc = CoreDocument::new(name.to_string(), DocumentTier::Constitutional, content.to_string());
        let sig = install_signing.sign(&doc.signable_bytes());
        doc.signature = BASE64.encode(sig.to_bytes());
        let base = name.strip_suffix(".md").unwrap_or(name);
        write_sig_file(&docs_dir, base, &doc).map_err(|e| e.to_string())?;
    }
    keyring.save().map_err(|e| e.to_string())?;
    Ok(())
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
fn verify_crypto(state: tauri::State<AppState>, docs_path: PathBuf) -> Result<(), String> {
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;
    b.verify_crypto(&docs_path).map_err(|e| e.to_string())
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
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;
    b.configure_email(
        &address,
        &imap_host,
        imap_port,
        &smtp_host,
        smtp_port,
        &password,
    )
    .map_err(|e| e.to_string())
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
    let downloader = abby_llm::ModelDownloader::new();
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
    Ok(())
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
fn list_skills(state: tauri::State<AppState>) -> Result<Vec<abby_skills::SkillManifest>, String> {
    state
        .registry
        .list()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_discovered_skills(state: tauri::State<AppState>) -> Result<Vec<abby_skills::SkillManifest>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let paths = vec![config.data_dir.join("skills")];
    Ok(abby_skills::SkillRegistry::discover(&paths))
}

#[tauri::command]
fn list_tools(
    state: tauri::State<AppState>,
    skill_id: String,
) -> Result<Vec<abby_skills::ToolDescriptor>, String> {
    let id = abby_skills::SkillId(skill_id);
    let (skill, _) = state.registry.get_skill(&id).map_err(|e| e.to_string())?;
    Ok(skill.tools())
}

#[tauri::command]
async fn execute_tool(
    state: tauri::State<'_, AppState>,
    skill_id: String,
    tool_name: String,
    params: HashMap<String, serde_json::Value>,
) -> Result<abby_skills::ToolOutput, String> {
    let id = abby_skills::SkillId(skill_id);
    let tool_params = ToolParams {
        values: params,
    };
    state
        .executor
        .execute(&id, &tool_name, tool_params)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat(state: tauri::State<'_, AppState>, message: String) -> Result<String, String> {
    // Get store and drop config lock before await
    let store = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        MemoryStore::open_with_config(&*config).map_err(|e| e.to_string())?
    };
    let messages = vec![abby_llm::Message {
        role: "user".to_string(),
        content: message.clone(),
    }];
    let response = state
        .router
        .route(messages.clone())
        .await
        .map_err(|e| e.to_string())?;
    let memory = Memory::ephemeral(format!("user: {} | assistant: {}", message, response.content));
    let _ = store.insert_memory(&memory);
    Ok(response.content)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = get_config();
    let router = IdEgoRouter::new(config.openai_api_key.clone());
    let registry = Arc::new(SkillRegistry::new());
    let event_bus = Arc::new(EventBus::new(256));
    let executor = Arc::new(SkillExecutor::new(registry.clone()));

    let state = AppState {
        config: RwLock::new(config),
        birth: RwLock::new(None),
        router,
        registry,
        executor,
        event_bus: event_bus.clone(),
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
            get_birth_stage,
            get_birth_message,
            start_birth,
            verify_crypto,
            configure_email,
            download_model,
            set_api_key,
            complete_birth,
            list_skills,
            list_discovered_skills,
            list_tools,
            execute_tool,
            chat,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
