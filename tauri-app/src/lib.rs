#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod commands;
pub mod identity_manager;
pub mod ollama_manager;
pub mod rate_limit;
pub mod state;
mod templates;

use crate::commands::agent::*;
use crate::commands::agentic::*;
use crate::commands::birth::*;
use crate::commands::chat::*;
use crate::commands::config::*;
use crate::commands::forge::*;
use crate::commands::identity::*;
use crate::commands::memory::*;
use crate::commands::sensory::*;
use crate::commands::skills::*;
use crate::state::AppState;

use abigail_auth::AuthManager;
use abigail_core::{validate_local_llm_url, AppConfig, SecretsVault};
use abigail_router::{IdEgoRouter, SubagentManager};
use abigail_skills::channel::EventBus;
use abigail_skills::{InstructionRegistry, SkillExecutor, SkillRegistry};
use identity_manager::IdentityManager;
use rate_limit::CooldownGuard;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use tauri::Manager;

/// Recursively copy a directory (for skill package install).
pub fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), String> {
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
pub fn skill_audit_log(data_dir: &Path, action: &str, detail: &str) {
    let log_path = data_dir.join("skill_audit.log");
    let line = format!(
        "{} {} {}\n",
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        action,
        detail
    );
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

fn get_config() -> AppConfig {
    let mut config = AppConfig::default_paths();
    let path = config.config_path();
    if path.exists() {
        config = AppConfig::load(&path).unwrap_or(config);
    }

    if let Some(ref url) = config.local_llm_base_url {
        if let Ok(normalized) = validate_local_llm_url(url) {
            config.local_llm_base_url = Some(normalized);
        } else {
            config.local_llm_base_url = None;
        }
    }

    if config.openai_api_key.is_none() {
        config.openai_api_key = std::env::var("OPENAI_API_KEY")
            .ok()
            .filter(|s| !s.is_empty());
    }

    config
}

pub fn extract_superego_config(config: &AppConfig) -> Option<(String, String)> {
    config.trinity.as_ref().and_then(|trinity| {
        match (&trinity.superego_provider, &trinity.superego_api_key) {
            (Some(provider), Some(key)) if !key.is_empty() => Some((provider.clone(), key.clone())),
            _ => None,
        }
    })
}

pub fn build_superego_llm_provider(
    provider: &str,
    key: &str,
) -> Arc<dyn abigail_capabilities::cognitive::LlmProvider> {
    let fallback =
        || match abigail_capabilities::cognitive::OpenAiProvider::new(Some(key.to_string())) {
            Ok(p) => Arc::new(p) as Arc<dyn abigail_capabilities::cognitive::LlmProvider>,
            Err(e) => {
                tracing::error!(
                    "Failed to create OpenAI fallback provider for Superego: {}",
                    e
                );
                Arc::new(abigail_capabilities::cognitive::CandleProvider::new())
                    as Arc<dyn abigail_capabilities::cognitive::LlmProvider>
            }
        };
    match provider {
        "anthropic" => {
            match abigail_capabilities::cognitive::AnthropicProvider::new(key.to_string()) {
                Ok(p) => Arc::new(p),
                Err(e) => {
                    tracing::error!("Failed to create Anthropic provider: {}", e);
                    fallback()
                }
            }
        }
        _ => fallback(),
    }
}

pub fn determine_ego_provider(
    config: &AppConfig,
    secrets: &SecretsVault,
) -> (Option<String>, Option<String>) {
    if let Some(trinity) = &config.trinity {
        if let Some(p) = &trinity.ego_provider {
            if let Some(k) = &trinity.ego_api_key {
                if !k.is_empty() {
                    return (Some(p.clone()), Some(k.clone()));
                }
            }
        }
    }

    if let Some(k) = &config.openai_api_key {
        if !k.is_empty() {
            return (Some("openai".to_string()), Some(k.clone()));
        }
    }

    let provider_names = ["anthropic", "openai", "xai", "perplexity", "google"];
    for name in &provider_names {
        if let Some(key) = secrets.get_secret(name) {
            let k = key.to_string();
            if !k.is_empty() {
                return (Some(name.to_string()), Some(k));
            }
        }
    }

    (None, None)
}

pub fn rebuild_router_with_superego(state: &AppState) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let vault = state.secrets.lock().map_err(|e| e.to_string())?;

    let (ego_name, ego_key) = {
        let (name, key) = determine_ego_provider(&config, &vault);
        if name.is_some() {
            (name, key)
        } else {
            let hive = state.hive_secrets.lock().map_err(|e| e.to_string())?;
            determine_ego_provider(&config, &hive)
        }
    };

    let mut new_router = IdEgoRouter::new(
        config.local_llm_base_url.clone(),
        ego_name.as_deref(),
        ego_key.clone(),
        config.routing_mode,
    );

    if let Some((se_provider, se_key)) = extract_superego_config(&config) {
        let superego = build_superego_llm_provider(&se_provider, &se_key);
        new_router = new_router.with_superego(superego);
    }

    let router_arc = Arc::new(new_router.clone());
    let mut router = state.router.write().map_err(|e| e.to_string())?;
    *router = new_router;
    drop(router);

    if let Ok(mut mgr) = state.subagent_manager.write() {
        mgr.update_router(router_arc);
    }

    Ok(())
}

pub fn rebuild_router_with_superego_from_handle(handle: &tauri::AppHandle) -> Result<(), String> {
    let state = handle.state::<AppState>();
    rebuild_router_with_superego(&state)
}

pub fn run() {
    let config = get_config();
    let data_dir = config.data_dir.clone();
    let registry = Arc::new(SkillRegistry::new());
    let executor = Arc::new(SkillExecutor::new(registry.clone()));
    let event_bus = Arc::new(EventBus::new(100));
    let secrets = Arc::new(Mutex::new(
        SecretsVault::load(data_dir.clone())
            .unwrap_or_else(|_| SecretsVault::new(data_dir.clone())),
    ));

    let hive_data_dir = abigail_core::AppConfig::default_paths().data_dir;
    let hive_secrets = Arc::new(Mutex::new(
        SecretsVault::load(hive_data_dir.clone())
            .unwrap_or_else(|_| SecretsVault::new(hive_data_dir.clone())),
    ));

    let (ego_name, ego_key) = determine_ego_provider(&config, &secrets.lock().unwrap());
    let router = IdEgoRouter::new(
        config.local_llm_base_url.clone(),
        ego_name.as_deref(),
        ego_key,
        config.routing_mode,
    );

    let auth_manager = Arc::new(AuthManager::new(secrets.clone()));
    let identity_manager =
        Arc::new(IdentityManager::new(hive_data_dir).expect("Failed to init IdentityManager"));
    let subagent_manager = RwLock::new(SubagentManager::new(Arc::new(router.clone())));

    let browser_config = abigail_capabilities::sensory::browser::BrowserCapabilityConfig::default();
    let browser = Arc::new(tokio::sync::RwLock::new(
        abigail_capabilities::sensory::browser::BrowserCapability::new(browser_config),
    ));
    let http_client = Arc::new(tokio::sync::RwLock::new(
        abigail_capabilities::sensory::http_client::HttpClientCapability::new(
            data_dir.join("downloads"),
        )
        .expect("Failed to init HttpClientCapability"),
    ));

    let state = AppState {
        config: RwLock::new(config),
        birth: RwLock::new(None),
        router: RwLock::new(router),
        registry,
        executor,
        event_bus,
        secrets,
        hive_secrets,
        auth_manager,
        identity_manager,
        active_agent_id: RwLock::new(None),
        subagent_manager,
        browser,
        http_client,
        ollama: Arc::new(tokio::sync::Mutex::new(None)),
        instruction_registry: Arc::new(InstructionRegistry::empty()),
        chat_cooldown: CooldownGuard::new(std::time::Duration::from_millis(500)),
        birth_cooldown: CooldownGuard::new(std::time::Duration::from_millis(500)),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            check_hive_status,
            get_identities,
            get_active_agent,
            load_agent,
            create_agent,
            disconnect_agent,
            migrate_legacy_identity,
            get_birth_complete,
            get_agent_name,
            get_docs_path,
            init_soul,
            generate_and_sign_constitutional,
            check_identity_status,
            check_interrupted_birth,
            repair_identity,
            get_birth_stage,
            get_birth_message,
            start_birth,
            verify_crypto,
            generate_identity,
            advance_past_darkness,
            advance_to_connectivity,
            complete_birth,
            list_skills,
            list_discovered_skills,
            list_tools,
            execute_tool,
            get_mcp_servers,
            mcp_list_tools,
            list_approved_skills,
            approve_skill,
            set_api_key,
            set_local_llm_url,
            get_router_status,
            set_superego_provider,
            get_entity_theme,
            get_stored_providers,
            set_active_provider,
            get_superego_l2_mode,
            set_superego_l2_mode,
            get_sqlite_stats,
            optimize_sqlite,
            reset_memories,
            search_memories,
            start_agentic_run,
            get_agentic_run_status,
            respond_to_mentor_ask,
            confirm_tool_execution,
            cancel_agentic_run,
            list_agentic_runs,
            list_subagents,
            delegate_to_subagent,
            get_governor_status,
            get_constraint_store,
            clear_constraints,
            upload_chat_attachment,
            get_forge_scenarios,
            crystallize_forge,
            genesis_chat,
            get_active_provider,
            chat,
            chat_stream,
            get_system_diagnostics,
            propose_entity_visuals,
            start_crystallization,
            extract_crystallization_identity,
            crystallize_soul,
            complete_emergence,
            sign_agent_with_hive,
            get_birth_transcript
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri app");
}
