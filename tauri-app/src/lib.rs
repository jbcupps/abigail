#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod commands;
pub mod hive_ops;
pub mod identity_manager;
pub mod log_capture;
pub mod ollama_manager;
pub mod rate_limit;
pub mod state;
mod templates;

pub mod probe;
pub mod skill_instructions;

use crate::commands::agent::*;
use crate::commands::agentic::*;
use crate::commands::birth::*;
use crate::commands::chat::*;
use crate::commands::cli::*;
use crate::commands::config::*;
use crate::commands::forge::*;
use crate::commands::identity::*;
use crate::commands::logging::*;
use crate::commands::memory::*;
use crate::commands::ollama::*;
use crate::commands::sensory::*;
use crate::commands::skills::*;
use crate::state::AppState;

use abigail_auth::AuthManager;
use abigail_core::{validate_local_llm_url, AppConfig, SecretsVault};
use abigail_hive::{Hive, ModelRegistry};
use abigail_router::{IdEgoRouter, SubagentManager};
use abigail_skills::channel::EventBus;
use abigail_skills::protocol::mcp::McpSkillRuntime;
use abigail_skills::{
    build_preloaded_skills, DynamicApiSkill, InstructionRegistry, ResourceLimits, Skill,
    SkillConfig, SkillExecutor, SkillRegistry, PRELOADED_SKILLS_VERSION,
};
use identity_manager::IdentityManager;
use rate_limit::CooldownGuard;
use std::collections::HashMap;
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

/// Build and optionally initialize the Proton Mail skill from current vault.
/// Used at startup and after storing IMAP-related secrets so the skill picks up new credentials.
/// Returns the skill (initialized or not) for the caller to register.
pub fn create_proton_mail_skill_for_registry(state: &AppState) -> Result<Arc<dyn Skill>, String> {
    use skill_proton_mail::ProtonMailSkill;
    let manifest = ProtonMailSkill::default_manifest();
    let mut skill = ProtonMailSkill::new(manifest);

    let has_creds = state
        .skills_secrets
        .lock()
        .map_err(|e| e.to_string())?
        .get_secret("imap_password")
        .is_some();

    if has_creds {
        let (imap_user, imap_password, imap_host, imap_port) = {
            let v = state.skills_secrets.lock().map_err(|e| e.to_string())?;
            (
                v.get_secret("imap_user").unwrap_or("").to_string(),
                v.get_secret("imap_password").unwrap_or("").to_string(),
                v.get_secret("imap_host")
                    .unwrap_or("mail.proton.me")
                    .to_string(),
                v.get_secret("imap_port").unwrap_or("993").to_string(),
            )
        };
        let imap_tls_mode = {
            let v = state.skills_secrets.lock().map_err(|e| e.to_string())?;
            v.get_secret("imap_tls_mode")
                .unwrap_or("IMPLICIT")
                .to_string()
        };

        let mut values = HashMap::new();
        values.insert(
            "imap_host".to_string(),
            serde_json::Value::String(imap_host),
        );
        values.insert(
            "imap_port".to_string(),
            serde_json::json!(imap_port.parse::<u64>().unwrap_or(993)),
        );
        values.insert(
            "imap_user".to_string(),
            serde_json::Value::String(imap_user),
        );
        values.insert(
            "imap_tls_mode".to_string(),
            serde_json::Value::String(imap_tls_mode),
        );

        let mut secrets = HashMap::new();
        secrets.insert("imap_password".to_string(), imap_password);

        let skill_config = SkillConfig {
            values,
            secrets,
            limits: ResourceLimits::default(),
            permissions: vec![],
            event_sender: Some(Arc::new(state.event_bus.sender())),
        };

        match tauri::async_runtime::block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(15),
                skill.initialize(skill_config),
            )
            .await
        }) {
            Ok(Ok(())) => {
                tracing::info!("Proton Mail skill initialized successfully");
            }
            Ok(Err(e)) => {
                tracing::warn!(
                    "Proton Mail skill init failed (registered uninitialized): {}",
                    e
                );
            }
            Err(_) => {
                tracing::warn!(
                    "Proton Mail skill init timed out after 15s (IMAP bridge unreachable?)"
                );
            }
        }
    } else {
        tracing::info!("Proton Mail skill created without credentials (no imap_password in vault)");
    }

    Ok(Arc::new(skill))
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

pub async fn rebuild_router(state: &AppState) -> Result<(), String> {
    // Capture the previous provider before rebuilding.
    let prev_provider = {
        let router = state.router.read().map_err(|e| e.to_string())?;
        router.ego_provider_name().map(|p| p.to_string())
    };

    // Resolve config synchronously (acquires only sync locks), then drop guards
    // before the async build_providers call.
    let hive_config = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        state.hive.resolve_config(&config)?
    };

    let built = abigail_hive::Hive::build_providers(&hive_config).await;

    let new_router = IdEgoRouter::from_built_providers(built);

    // Only update the timestamp when the provider actually changed.
    let new_provider = new_router.ego_provider_name().map(|p| p.to_string());
    if new_provider != prev_provider {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.last_provider_change_at = Some(chrono::Utc::now().to_rfc3339());
        let _ = config.save(&config.config_path());
    }

    let router_arc = Arc::new(new_router.clone());
    let mut router = state.router.write().map_err(|e| e.to_string())?;
    *router = new_router;
    drop(router);

    if let Ok(mut mgr) = state.subagent_manager.write() {
        mgr.update_router(router_arc);
    }

    // Background model discovery via ModelRegistry (non-blocking).
    // Refreshes the active ego provider's model list and persists to config.
    let ego_provider = hive_config.ego_provider_name.clone();
    let ego_key = hive_config.ego_api_key.clone();
    let registry_handle = state.model_registry.clone();
    tokio::spawn(async move {
        if let (Some(provider), Some(key)) = (ego_provider, ego_key) {
            let mut reg = registry_handle.lock().await;
            match reg.refresh_provider(&provider, &key).await {
                Ok(cache) => {
                    tracing::info!(
                        "ModelRegistry: discovered {} model(s) from {}",
                        cache.models.len(),
                        provider
                    );
                    for m in cache.models.iter().take(5) {
                        tracing::info!("  - {}", m.id);
                    }
                }
                Err(e) => {
                    tracing::warn!("ModelRegistry: discovery failed for {}: {}", provider, e);
                }
            }
        }
    });

    Ok(())
}

pub async fn rebuild_router_from_handle(handle: &tauri::AppHandle) -> Result<(), String> {
    let state = handle.state::<AppState>();
    rebuild_router(&state).await
}

pub fn run() {
    if probe::should_run() {
        probe::run_and_exit();
    }

    let log_buffer = log_capture::new_log_buffer();
    log_capture::init_tracing(log_buffer.clone());

    let config = get_config();
    let data_dir = config.data_dir.clone();
    let secrets = Arc::new(Mutex::new(
        SecretsVault::load(data_dir.clone())
            .unwrap_or_else(|_| SecretsVault::new(data_dir.clone())),
    ));

    let skills_secrets = Arc::new(Mutex::new(
        SecretsVault::load_custom(data_dir.clone(), "skills.bin")
            .unwrap_or_else(|_| SecretsVault::new_custom(data_dir.clone(), "skills.bin")),
    ));
    let registry = Arc::new(SkillRegistry::with_secrets(skills_secrets.clone()));
    let executor = Arc::new(SkillExecutor::new(registry.clone()));
    let event_bus = Arc::new(EventBus::new(100));

    let hive_data_dir = abigail_core::AppConfig::default_paths().data_dir;
    let hive_secrets = Arc::new(Mutex::new(
        SecretsVault::load(hive_data_dir.clone())
            .unwrap_or_else(|_| SecretsVault::new(hive_data_dir.clone())),
    ));

    let hive = Arc::new(Hive::new(secrets.clone(), hive_secrets.clone()));

    let router = tauri::async_runtime::block_on(async {
        let built = hive
            .build_providers_from_config(&config)
            .await
            .expect("Failed to build initial providers");
        IdEgoRouter::from_built_providers(built)
    });

    // Initialize model registry from persisted catalog
    let model_registry = {
        let mut reg = ModelRegistry::new();
        reg.load_from_catalog(&config.provider_catalog);
        if reg.total_models() > 0 {
            tracing::info!(
                "ModelRegistry: loaded {} model(s) across {} provider(s) from persisted catalog",
                reg.total_models(),
                reg.providers().len()
            );
        }
        Arc::new(tokio::sync::Mutex::new(reg))
    };

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

    // Seed skill instructions into data_dir when absent (first run / clean install).
    skill_instructions::bootstrap_if_needed(&data_dir);

    let state = AppState {
        config: RwLock::new(config),
        birth: RwLock::new(None),
        router: RwLock::new(router),
        registry,
        executor,
        event_bus,
        secrets,
        skills_secrets,
        hive_secrets,
        hive,
        auth_manager,
        identity_manager,
        active_agent_id: RwLock::new(None),
        subagent_manager,
        browser,
        http_client,
        ollama: Arc::new(tokio::sync::Mutex::new(None)),
        model_registry,
        instruction_registry: Arc::new({
            let skills_dir = data_dir.join("skills");
            let registry_path = skills_dir.join("registry.toml");
            let instructions_dir = skills_dir.join("instructions");
            if registry_path.exists() {
                InstructionRegistry::load(&registry_path, &instructions_dir)
            } else {
                InstructionRegistry::empty()
            }
        }),
        chat_cooldown: CooldownGuard::new(std::time::Duration::from_millis(500)),
        birth_cooldown: CooldownGuard::new(std::time::Duration::from_millis(500)),
        cli_server: Arc::new(tokio::sync::Mutex::new(None)),
        log_buffer: log_buffer.clone(),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let handle = app.handle();
            let state = handle.state::<AppState>();

            // Register Hive Management Skill
            let hive_ops = Arc::new(crate::hive_ops::TauriHiveOps::new(handle.clone()));
            let hive_skill = Arc::new(abigail_skills::hive::HiveManagementSkill::new(hive_ops));
            state
                .registry
                .register(
                    abigail_skills::manifest::SkillId("builtin.hive_management".to_string()),
                    hive_skill,
                )
                .map_err(|e| e.to_string())?;

            // Register Skill Factory with registry + secrets for immediate re-registration
            let skills_dir = state.config.read().unwrap().data_dir.join("skills");
            let factory_skill = Arc::new(
                abigail_skills::factory::SkillFactory::new(skills_dir)
                    .with_registry(state.registry.clone())
                    .with_secrets(state.skills_secrets.clone()),
            );
            state
                .registry
                .register(
                    abigail_skills::manifest::SkillId("builtin.skill_factory".to_string()),
                    factory_skill,
                )
                .map_err(|e| e.to_string())?;

            // Bootstrap preloaded dynamic skills when embedded version advances.
            {
                let preloaded = build_preloaded_skills(Some(state.skills_secrets.clone()));
                for skill in preloaded {
                    let skill_id = skill.manifest().id.clone();
                    state
                        .registry
                        .register(skill_id, Arc::new(skill))
                        .map_err(|e| e.to_string())?;
                }

                let needs_bootstrap = {
                    let cfg = state.config.read().map_err(|e| e.to_string())?;
                    cfg.preloaded_skills_version < PRELOADED_SKILLS_VERSION
                };
                if needs_bootstrap {
                    let mut cfg = state.config.write().map_err(|e| e.to_string())?;
                    cfg.preloaded_skills_version = PRELOADED_SKILLS_VERSION;
                    cfg.save(&cfg.config_path()).map_err(|e| e.to_string())?;
                }
            }

            // Discover runtime dynamic API skills from data_dir/skills/*.json
            {
                let cfg = state.config.read().map_err(|e| e.to_string())?;
                let dynamic_skills = DynamicApiSkill::discover(
                    &cfg.data_dir.join("skills"),
                    Some(state.skills_secrets.clone()),
                );
                drop(cfg);
                for skill in dynamic_skills {
                    let skill_id = skill.manifest().id.clone();
                    state
                        .registry
                        .register(skill_id, Arc::new(skill))
                        .map_err(|e| e.to_string())?;
                }
            }

            // Register and initialize Proton Mail (IMAP) skill.
            // Mirrors entity-daemon: always registers the skill (so its manifest
            // declares imap_*/smtp_* secrets for namespace validation), and
            // initializes the IMAP transport only when credentials are present.
            {
                let skill_id = skill_proton_mail::ProtonMailSkill::default_manifest()
                    .id
                    .clone();
                match create_proton_mail_skill_for_registry(&state) {
                    Ok(skill) => {
                        if let Err(e) = state.registry.register(skill_id, skill) {
                            tracing::warn!("Failed to register Proton Mail skill: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Proton Mail skill creation failed: {}", e);
                    }
                }
            }

            // Register native Rust skills (compiled into the binary).
            {
                let data_dir = {
                    let cfg = state.config.read().map_err(|e| e.to_string())?;
                    cfg.data_dir.clone()
                };
                let skills_secrets = state.skills_secrets.clone();
                let allowed_roots = vec![data_dir.clone()];

                macro_rules! register_skill {
                    ($skill:expr) => {{
                        let s = $skill;
                        let id = s.manifest().id.clone();
                        if let Err(e) = state.registry.register(id.clone(), Arc::new(s)) {
                            tracing::warn!("Failed to register {}: {}", id.0, e);
                        }
                    }};
                }

                register_skill!(skill_clipboard::ClipboardSkill::new(
                    skill_clipboard::ClipboardSkill::default_manifest()
                ));
                register_skill!(skill_shell::ShellSkill::new(
                    skill_shell::ShellSkill::default_manifest()
                ));
                register_skill!(skill_git::GitSkill::new(
                    skill_git::GitSkill::default_manifest()
                ));
                register_skill!(skill_notification::NotificationSkill::new(
                    skill_notification::NotificationSkill::default_manifest()
                ));
                register_skill!(skill_system_monitor::SystemMonitorSkill::new(
                    skill_system_monitor::SystemMonitorSkill::default_manifest()
                ));
                register_skill!(skill_http::HttpSkill::new(
                    skill_http::HttpSkill::default_manifest()
                ));
                register_skill!(skill_calendar::CalendarSkill::new(
                    skill_calendar::CalendarSkill::default_manifest(),
                    data_dir.clone()
                ));
                register_skill!(skill_knowledge_base::KnowledgeBaseSkill::new(
                    skill_knowledge_base::KnowledgeBaseSkill::default_manifest(),
                    data_dir.clone()
                ));
                register_skill!(skill_filesystem::FilesystemSkill::new(
                    skill_filesystem::FilesystemSkill::default_manifest(),
                    allowed_roots.clone()
                ));
                register_skill!(skill_database::DatabaseSkill::new(
                    skill_database::DatabaseSkill::default_manifest(),
                    allowed_roots.clone()
                ));
                register_skill!(skill_code_analysis::CodeAnalysisSkill::new(
                    skill_code_analysis::CodeAnalysisSkill::default_manifest(),
                    allowed_roots.clone()
                ));
                register_skill!(skill_document::DocumentSkill::new(
                    skill_document::DocumentSkill::default_manifest(),
                    allowed_roots.clone()
                ));
                register_skill!(skill_image::ImageSkill::new(
                    skill_image::ImageSkill::default_manifest(),
                    allowed_roots.clone()
                ));
                register_skill!(skill_web_search::WebSearchSkill::with_secrets(
                    skill_web_search::WebSearchSkill::default_manifest(),
                    skills_secrets.clone()
                ));
                register_skill!(skill_perplexity_search::PerplexitySearchSkill::with_secrets(
                    skill_perplexity_search::PerplexitySearchSkill::default_manifest(),
                    skills_secrets.clone()
                ));
            }

            // Register configured MCP servers as skills (HTTP transport).
            {
                let servers = {
                    let cfg = state.config.read().map_err(|e| e.to_string())?;
                    cfg.mcp_servers.clone()
                };
                for server in servers
                    .into_iter()
                    .filter(|s| s.transport.eq_ignore_ascii_case("http"))
                {
                    let mut runtime = McpSkillRuntime::new(
                        format!("mcp.{}", server.id),
                        format!("MCP {}", server.name),
                        server.command_or_url.clone(),
                    );
                    let init = tauri::async_runtime::block_on(async {
                        runtime
                            .initialize(SkillConfig {
                                values: HashMap::new(),
                                secrets: HashMap::new(),
                                limits: ResourceLimits::default(),
                                permissions: vec![],
                                event_sender: Some(Arc::new(state.event_bus.sender())),
                            })
                            .await
                    });
                    if let Err(e) = init {
                        tracing::warn!(
                            "Failed to initialize MCP runtime for server {}: {}",
                            server.id,
                            e
                        );
                    }
                    let skill_id = runtime.manifest().id.clone();
                    state
                        .registry
                        .register(skill_id, Arc::new(runtime))
                        .map_err(|e| e.to_string())?;
                }
            }

            Ok(())
        })
        .manage(state)
        .manage(log_buffer)
        .invoke_handler(tauri::generate_handler![
            run_startup_checks,
            check_hive_status,
            get_identities,
            get_active_agent,
            load_agent,
            create_agent,
            reset_birth,
            delete_agent_identity,
            archive_agent_identity,
            disconnect_agent,
            suspend_agent,
            save_recovery_key,
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
            advance_to_crystallization,
            get_genesis_paths,
            complete_birth,
            birth_chat,
            list_skills,
            list_discovered_skills,
            list_missing_skill_secrets,
            list_skills_vault_entries,
            store_secret,
            list_tools,
            execute_tool,
            get_mcp_servers,
            mcp_list_tools,
            list_approved_skills,
            approve_skill,
            list_signed_skill_allowlist,
            upsert_signed_skill_allowlist_entry,
            revoke_signed_skill_allowlist_entry,
            get_cli_server_status,
            start_cli_server,
            stop_cli_server,
            set_api_key,
            set_local_llm_url,
            store_provider_key,
            get_router_status,
            diagnose_routing,
            detect_ollama,
            list_recommended_models,
            install_ollama,
            pull_ollama_model,
            get_ollama_status,
            probe_local_llm,
            set_local_llm_during_birth,
            use_stored_provider,
            get_entity_theme,
            get_identity_sharing_settings,
            set_identity_sharing_settings,
            get_visual_adaptation_settings,
            set_visual_adaptation_settings,
            get_memory_disclosure_settings,
            set_memory_disclosure_settings,
            get_forge_ui_settings,
            set_forge_advanced_mode,
            get_stored_providers,
            set_active_provider,
            get_active_provider,
            set_ego_model,
            get_ego_model,
            set_routing_mode,
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
            preview_forge_primary_intelligence,
            apply_forge_primary_intelligence,
            forge_undo_last_change,
            get_forge_audit_events,
            get_forge_undo_status,
            genesis_chat,
            chat,
            chat_stream,
            get_system_diagnostics,
            get_log_level,
            set_log_level,
            get_captured_logs,
            clear_captured_logs,
            export_logs,
            save_logs_to_file,
            propose_entity_visuals,
            start_crystallization,
            extract_crystallization_identity,
            crystallize_soul,
            complete_emergence,
            sign_agent_with_hive,
            get_birth_transcript,
            get_model_registry,
            discover_provider_models,
            refresh_model_registry,
            get_force_override,
            set_force_override,
            get_tier_thresholds,
            set_tier_thresholds,
            get_tier_models,
            set_tier_model,
            reset_tier_models
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri app");
}
