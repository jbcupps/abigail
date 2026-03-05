#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod agentic_runtime;
pub mod backup_ops;
pub mod chat_coordinator;
pub mod commands;
pub mod daemon_manager;
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
use crate::commands::jobs::*;
use crate::commands::logging::*;
use crate::commands::memory::*;
use crate::commands::ollama::*;
use crate::commands::orchestration::*;
use crate::commands::sensory::*;
use crate::commands::skills::*;
use crate::state::AppState;

use abigail_auth::AuthManager;
use abigail_core::{validate_local_llm_url, AppConfig, SecretsVault};
use abigail_hive::{Hive, ModelRegistry};
use abigail_memory::MemoryStore;
#[allow(deprecated)]
use abigail_router::{
    IdEgoRouter, OrchestrationScheduler, SubagentDefinition, SubagentManager, SubagentProvider,
};
use abigail_skills::protocol::mcp::McpSkillRuntime;
use abigail_skills::{
    build_preloaded_skills, DynamicApiSkill, InstructionRegistry, ResourceLimits, Skill,
    SkillConfig, SkillExecutionPolicy, SkillExecutor, SkillRegistry, SkillsWatcher,
    PRELOADED_SKILLS_VERSION,
};
use abigail_streaming::MemoryBroker;
use identity_manager::IdentityManager;
use rate_limit::CooldownGuard;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use tauri::{Emitter, Manager};

/// Show a native OS error dialog (blocking) so users see startup failures
/// even when running as a Windows GUI app with no console.
pub fn show_fatal_error(title: &str, message: &str) {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        fn to_wide(s: &str) -> Vec<u16> {
            OsStr::new(s)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        }

        let title_w = to_wide(title);
        let msg_w = to_wide(message);

        // MB_OK | MB_ICONERROR
        const MB_OK: u32 = 0x0000_0000;
        const MB_ICONERROR: u32 = 0x0000_0010;

        unsafe {
            #[link(name = "user32")]
            extern "system" {
                fn MessageBoxW(
                    hwnd: *const (),
                    text: *const u16,
                    caption: *const u16,
                    utype: u32,
                ) -> i32;
            }
            MessageBoxW(
                std::ptr::null(),
                msg_w.as_ptr(),
                title_w.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        // On non-Windows, print to stderr (best effort).
        eprintln!("[{title}] {message}");
    }
}

/// Install a panic hook that shows a native dialog before aborting.
/// Call this once, early in main(), before any work that might panic.
pub fn install_panic_dialog_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown error".to_string()
        };

        let location = info
            .location()
            .map(|loc| {
                format!(
                    "\n\nLocation: {}:{}:{}",
                    loc.file(),
                    loc.line(),
                    loc.column()
                )
            })
            .unwrap_or_default();

        let message = format!(
            "Abigail failed to start:\n\n{payload}{location}\n\n\
             If this keeps happening, try deleting the data folder at:\n\
             %LOCALAPPDATA%\\abigail\\Abigail\\\n\
             and reinstalling."
        );

        show_fatal_error("Abigail — Startup Error", &message);

        // Also call the default hook so it prints to stderr (useful when
        // launched from a terminal).
        default_hook(info);
    }));
}

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

/// Build and optionally initialize the Email skill from current vault.
/// Used at startup and after storing IMAP-related secrets so the skill picks up new credentials.
/// Returns the skill (initialized or not) for the caller to register.
pub fn create_email_skill_for_registry(state: &AppState) -> Result<Arc<dyn Skill>, String> {
    use skill_email::EmailSkill;
    let manifest = EmailSkill::default_manifest();
    let mut skill = EmailSkill::new(manifest);

    let has_creds = state
        .skills_secrets
        .lock()
        .map_err(|e| e.to_string())?
        .get_secret("imap_password")
        .is_some();

    if has_creds {
        let (imap_user, imap_password, imap_host, imap_port, imap_tls_mode, smtp_host, smtp_port) = {
            let v = state.skills_secrets.lock().map_err(|e| e.to_string())?;
            (
                v.get_secret("imap_user").unwrap_or("").to_string(),
                v.get_secret("imap_password").unwrap_or("").to_string(),
                v.get_secret("imap_host").unwrap_or("").to_string(),
                v.get_secret("imap_port").unwrap_or("993").to_string(),
                v.get_secret("imap_tls_mode")
                    .unwrap_or("IMPLICIT")
                    .to_string(),
                v.get_secret("smtp_host").unwrap_or("").to_string(),
                v.get_secret("smtp_port").unwrap_or("587").to_string(),
            )
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
        if !smtp_host.is_empty() {
            values.insert(
                "smtp_host".to_string(),
                serde_json::Value::String(smtp_host),
            );
        }
        if !smtp_port.is_empty() {
            values.insert(
                "smtp_port".to_string(),
                serde_json::json!(smtp_port.parse::<u64>().unwrap_or(587)),
            );
        }

        let mut secrets = HashMap::new();
        secrets.insert("imap_password".to_string(), imap_password);

        let skill_config = SkillConfig {
            values,
            secrets,
            limits: ResourceLimits::default(),
            permissions: vec![],
            stream_broker: Some(state.stream_broker.clone()),
        };

        match tauri::async_runtime::block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(15),
                skill.initialize(skill_config),
            )
            .await
        }) {
            Ok(Ok(())) => {
                tracing::info!("Email skill initialized successfully");
            }
            Ok(Err(e)) => {
                tracing::warn!("Email skill init failed (registered uninitialized): {}", e);
            }
            Err(_) => {
                tracing::warn!("Email skill init timed out after 15s (IMAP server unreachable?)");
            }
        }
    } else {
        tracing::info!("Email skill created without credentials (no imap_password in vault)");
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

fn register_runtime_subagents(state: &AppState) -> Result<(), String> {
    let mut manager = state.subagent_manager.write().map_err(|e| e.to_string())?;

    manager.register(SubagentDefinition {
        id: "research_specialist".to_string(),
        name: "Research Specialist".to_string(),
        description: "Handles research synthesis and documentation-heavy investigative tasks."
            .to_string(),
        capabilities: vec![
            "web_search".to_string(),
            "knowledge_base".to_string(),
            "document".to_string(),
        ],
        provider: SubagentProvider::SameAsEgo,
    });

    manager.register(SubagentDefinition {
        id: "code_operations".to_string(),
        name: "Code Operations".to_string(),
        description:
            "Focused on repository analysis, shell tasks, and code-level implementation work."
                .to_string(),
        capabilities: vec![
            "code_analysis".to_string(),
            "filesystem".to_string(),
            "git".to_string(),
            "shell".to_string(),
        ],
        provider: SubagentProvider::SameAsEgo,
    });

    manager.register(SubagentDefinition {
        id: "local_guardian".to_string(),
        name: "Local Guardian".to_string(),
        description:
            "Runs local safety and diagnostics checks that should stay on the local provider."
                .to_string(),
        capabilities: vec!["diagnostics".to_string(), "system_monitor".to_string()],
        provider: SubagentProvider::SameAsId,
    });

    Ok(())
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
    if let Err(e) = try_run() {
        show_fatal_error(
            "Abigail — Startup Error",
            &format!(
                "Abigail failed to start:\n\n{e}\n\n\
             If this keeps happening, try deleting the data folder at:\n\
             %LOCALAPPDATA%\\abigail\\Abigail\\\n\
             and reinstalling."
            ),
        );
        std::process::exit(1);
    }
}

fn try_run() -> Result<(), String> {
    if probe::should_run() {
        probe::run_and_exit();
    }

    let log_buffer = log_capture::new_log_buffer();
    log_capture::init_tracing(log_buffer.clone());

    let config = get_config();
    let data_dir = config.data_dir.clone();
    let iggy_connection = config.iggy_connection.clone();
    let secrets = Arc::new(Mutex::new(
        SecretsVault::load(data_dir.clone())
            .unwrap_or_else(|_| SecretsVault::new(data_dir.clone())),
    ));

    let skills_secrets = Arc::new(Mutex::new(
        SecretsVault::load_custom(data_dir.clone(), "skills.bin")
            .unwrap_or_else(|_| SecretsVault::new_custom(data_dir.clone(), "skills.bin")),
    ));
    let registry = Arc::new(SkillRegistry::with_secrets(skills_secrets.clone()));
    if let Err(e) = registry.set_execution_policy(SkillExecutionPolicy::from_app_config(&config)) {
        tracing::error!("Failed to apply initial skill execution policy: {}", e);
    }
    let executor = Arc::new(SkillExecutor::new(registry.clone()));
    let stream_broker: Arc<dyn abigail_streaming::StreamBroker> = Arc::new(MemoryBroker::default());

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
            .map_err(|e| format!("Failed to build LLM providers: {e}"))?;
        Ok::<_, String>(IdEgoRouter::from_built_providers(built))
    })?;

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
    let identity_manager = Arc::new(
        IdentityManager::new(hive_data_dir)
            .map_err(|e| format!("Failed to init IdentityManager: {e}"))?,
    );
    let subagent_manager = RwLock::new(SubagentManager::new(Arc::new(router.clone())));

    let browser_config = abigail_capabilities::sensory::browser::BrowserCapabilityConfig::default();
    let browser = Arc::new(tokio::sync::RwLock::new(
        abigail_capabilities::sensory::browser::BrowserCapability::new(browser_config),
    ));
    let http_client = Arc::new(tokio::sync::RwLock::new(
        abigail_capabilities::sensory::http_client::HttpClientCapability::new(
            data_dir.join("downloads"),
        )
        .map_err(|e| format!("Failed to init HttpClientCapability: {e}"))?,
    ));

    // Open shared MemoryStore for chat persistence and memory queries.
    // Wrapped in RwLock so load_agent can swap to the per-entity DB.
    let memory = Arc::new(std::sync::RwLock::new(
        MemoryStore::open_with_config(&config)
            .map_err(|e| format!("Failed to open MemoryStore: {e}"))?,
    ));
    let agentic_runtime = Arc::new(agentic_runtime::AgenticRuntime::new(&data_dir));
    #[allow(deprecated)]
    let orchestration_scheduler = Arc::new(OrchestrationScheduler::new(data_dir.clone()));

    // Open job queue database for async task management.
    let job_queue = {
        let job_db_path = data_dir.join("jobs.db");
        let conn = rusqlite::Connection::open(&job_db_path).map_err(|e| {
            format!(
                "Failed to open job queue database at {}: {e}",
                job_db_path.display()
            )
        })?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| format!("Failed to set WAL mode on job queue: {e}"))?;
        conn.execute_batch(abigail_queue::MIGRATION_V3_JOB_QUEUE)
            .map_err(|e| format!("Failed to run job queue migrations: {e}"))?;
        for stmt in abigail_queue::MIGRATION_V4_ORCHESTRATION.split(';') {
            let trimmed = stmt.trim();
            if !trimmed.is_empty() {
                let _ = conn.execute_batch(trimmed);
            }
        }
        for stmt in abigail_queue::MIGRATION_V5_DEPENDS_ON.split(';') {
            let trimmed = stmt.trim();
            if !trimmed.is_empty() {
                let _ = conn.execute_batch(trimmed);
            }
        }
        for stmt in abigail_queue::MIGRATION_V6_EXECUTION_MODE.split(';') {
            let trimmed = stmt.trim();
            if !trimmed.is_empty() {
                let _ = conn.execute_batch(trimmed);
            }
        }
        Arc::new(abigail_queue::JobQueue::new(
            Arc::new(std::sync::Mutex::new(conn)),
            stream_broker.clone(),
        ))
    };

    // Seed skill instructions into data_dir when absent (first run / clean install).
    skill_instructions::bootstrap_if_needed(&data_dir);

    let state = AppState {
        config: RwLock::new(config),
        birth: RwLock::new(None),
        router: RwLock::new(router),
        registry,
        executor,
        stream_broker,
        secrets,
        skills_secrets,
        hive_secrets,
        hive,
        auth_manager,
        identity_manager,
        memory,
        active_agent_id: RwLock::new(None),
        subagent_manager,
        agentic_runtime,
        orchestration_scheduler,
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
        active_chat_cancel: Arc::new(tokio::sync::Mutex::new(None)),
        constraints: Arc::new(std::sync::RwLock::new(
            abigail_router::ConstraintStore::with_data_dir(data_dir.clone()),
        )),
        job_queue,
        daemon_manager: Arc::new(tokio::sync::Mutex::new(
            daemon_manager::DaemonManager::new(data_dir.clone()).with_iggy(iggy_connection),
        )),
        force_override: RwLock::new(crate::state::ForceOverride::default()),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let handle = app.handle();
            let state = handle.state::<AppState>();

            tauri::async_runtime::block_on(async {
                state
                    .agentic_runtime
                    .initialize_recovery()
                    .await
                    .map_err(|e| e.to_string())
            })?;

            register_runtime_subagents(&state)?;

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

            // Register Backup Management Skill
            let backup_ops = Arc::new(crate::backup_ops::TauriBackupOps::new(handle.clone()));
            let backup_skill = Arc::new(abigail_skills::backup::BackupManagementSkill::new(
                backup_ops,
            ));
            state
                .registry
                .register(
                    abigail_skills::manifest::SkillId("builtin.backup_management".to_string()),
                    backup_skill,
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

            // Register and initialize Email (IMAP/SMTP) skill.
            // Mirrors entity-daemon: always registers the skill (so its manifest
            // declares imap_*/smtp_* secrets for namespace validation), and
            // initializes the IMAP transport only when credentials are present.
            {
                let skill_id = skill_email::EmailSkill::default_manifest().id.clone();
                match create_email_skill_for_registry(&state) {
                    Ok(skill) => {
                        if let Err(e) = state.registry.register(skill_id, skill) {
                            tracing::warn!("Failed to register Email skill: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Email skill creation failed: {}", e);
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
                let mut allowed_roots = vec![data_dir.clone()];
                // Allow scratch work in system temp dir
                allowed_roots.push(std::env::temp_dir());
                // Allow user-facing outputs in Documents/Abigail/
                if let Some(docs_dir) = directories::UserDirs::new()
                    .and_then(|u| u.document_dir().map(|d| d.to_path_buf()))
                {
                    allowed_roots.push(docs_dir.join("Abigail"));
                }

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
                register_skill!(
                    skill_perplexity_search::PerplexitySearchSkill::with_secrets(
                        skill_perplexity_search::PerplexitySearchSkill::default_manifest(),
                        skills_secrets.clone()
                    )
                );
            }

            // Register configured MCP servers as skills (HTTP transport).
            {
                let (servers, trust_policy, data_dir) = {
                    let cfg = state.config.read().map_err(|e| e.to_string())?;
                    (
                        cfg.mcp_servers.clone(),
                        cfg.mcp_trust_policy.clone(),
                        cfg.data_dir.clone(),
                    )
                };
                for server in servers
                    .into_iter()
                    .filter(|s| s.transport.eq_ignore_ascii_case("http"))
                {
                    if let Err(policy_err) =
                        trust_policy.validate_http_server_url(&server.id, &server.command_or_url)
                    {
                        tracing::warn!("{}", policy_err);
                        skill_audit_log(
                            &data_dir,
                            "mcp_trust_deny",
                            &format!("server_id={} reason={}", server.id, policy_err),
                        );
                        continue;
                    }

                    let mut runtime = McpSkillRuntime::new(
                        format!("mcp.{}", server.id),
                        format!("MCP {}", server.name),
                        server.command_or_url.clone(),
                        Some(trust_policy.clone()),
                    );
                    let init = tauri::async_runtime::block_on(async {
                        runtime
                            .initialize(SkillConfig {
                                values: HashMap::new(),
                                secrets: HashMap::new(),
                                limits: ResourceLimits::default(),
                                permissions: vec![],
                                stream_broker: Some(state.stream_broker.clone()),
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

            // Spawn SkillsWatcher for hot-reload of dynamic skills
            {
                let skills_dir = {
                    let cfg = state.config.read().map_err(|e| e.to_string())?;
                    cfg.data_dir.join("skills")
                };
                let registry_for_watcher = state.registry.clone();
                let vault_for_watcher = Some(state.skills_secrets.clone());
                let broker_for_watcher = state.stream_broker.clone();
                let app_handle = handle.clone();
                match SkillsWatcher::start(vec![skills_dir]) {
                    Ok((watcher, mut rx)) => {
                        // Keep the watcher alive by leaking the handle (dropped on app exit)
                        std::mem::forget(watcher);
                        tauri::async_runtime::spawn(async move {
                            while let Ok(event) = rx.recv().await {
                                match event {
                                    abigail_skills::SkillFileEvent::Changed(path) => {
                                        tracing::info!(
                                            "Skill watcher: detected change at {:?}",
                                            path
                                        );
                                        let dir = if path.is_file() {
                                            path.parent().map(|p| p.to_path_buf())
                                        } else {
                                            Some(path.clone())
                                        };
                                        if let Some(parent) = dir {
                                            for entry in std::fs::read_dir(parent)
                                                .into_iter()
                                                .flatten()
                                                .flatten()
                                            {
                                                let p = entry.path();
                                                if p.extension().and_then(|e| e.to_str())
                                                    == Some("json")
                                                {
                                                    match DynamicApiSkill::load_from_path(
                                                        &p,
                                                        vault_for_watcher.clone(),
                                                    ) {
                                                        Ok(skill) => {
                                                            let sid =
                                                                abigail_skills::manifest::SkillId(
                                                                    skill
                                                                        .manifest()
                                                                        .id
                                                                        .0
                                                                        .clone(),
                                                                );
                                                            let _ = registry_for_watcher.register(
                                                                sid.clone(),
                                                                Arc::new(skill),
                                                            );
                                                            tracing::info!(
                                                                "Skill watcher: registered {}",
                                                                sid.0
                                                            );
                                                            let _ = app_handle.emit(
                                                                "skill-reloaded",
                                                                serde_json::json!({
                                                                    "skill_id": sid.0,
                                                                    "path": p.display().to_string()
                                                                }),
                                                            );
                                                            abigail_skills::channel::event::publish_skill_event(
                                                                &broker_for_watcher,
                                                                abigail_skills::channel::event::SkillEvent {
                                                                    skill_id: sid,
                                                                    trigger: "skill_reloaded".to_string(),
                                                                    payload: serde_json::json!({ "path": p.display().to_string() }),
                                                                    timestamp: chrono::Utc::now(),
                                                                    priority: abigail_skills::channel::TriggerPriority::Normal,
                                                                },
                                                            ).await;
                                                        }
                                                        Err(e) => tracing::debug!(
                                                            "Skill watcher: skip {:?}: {}",
                                                            p,
                                                            e
                                                        ),
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    abigail_skills::SkillFileEvent::Removed(path) => {
                                        tracing::info!(
                                            "Skill watcher: skill removed at {:?}",
                                            path
                                        );
                                        if let Some(parent) = path.parent() {
                                            for entry in std::fs::read_dir(parent)
                                                .into_iter()
                                                .flatten()
                                                .flatten()
                                            {
                                                let p = entry.path();
                                                if p.extension().and_then(|e| e.to_str())
                                                    == Some("json")
                                                {
                                                    if let Ok(bytes) =
                                                        std::fs::read_to_string(&p)
                                                    {
                                                        if let Ok(cfg) = serde_json::from_str::<
                                                            abigail_skills::dynamic::DynamicSkillConfig,
                                                        >(
                                                            &bytes
                                                        ) {
                                                            let sid =
                                                                abigail_skills::manifest::SkillId(
                                                                    cfg.id.clone(),
                                                                );
                                                            let _ = registry_for_watcher
                                                                .unregister(&sid);
                                                            tracing::info!(
                                                                "Skill watcher: unregistered {}",
                                                                sid.0
                                                            );
                                                            let _ = app_handle.emit(
                                                                "skill-removed",
                                                                serde_json::json!({
                                                                    "skill_id": sid.0,
                                                                    "path": p.display().to_string()
                                                                }),
                                                            );
                                                            abigail_skills::channel::event::publish_skill_event(
                                                                &broker_for_watcher,
                                                                abigail_skills::channel::event::SkillEvent {
                                                                    skill_id: sid,
                                                                    trigger: "skill_removed".to_string(),
                                                                    payload: serde_json::json!({ "path": p.display().to_string() }),
                                                                    timestamp: chrono::Utc::now(),
                                                                    priority: abigail_skills::channel::TriggerPriority::Normal,
                                                                },
                                                            ).await;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    abigail_skills::SkillFileEvent::RegistryChanged(path) => {
                                        tracing::info!(
                                            "Skill watcher: registry changed at {:?} (Tauri runtime does not re-provision persistent topology)",
                                            path
                                        );
                                    }
                                    abigail_skills::SkillFileEvent::RegistryRemoved(path) => {
                                        tracing::warn!(
                                            "Skill watcher: registry removed at {:?}",
                                            path
                                        );
                                    }
                                }
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Failed to start skills watcher: {}", e);
                    }
                }
            }

            // Relay JobQueue local broadcast events to the Tauri frontend.
            {
                let mut job_rx = state.job_queue.subscribe_local();
                let app_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    while let Ok(event) = job_rx.recv().await {
                        if let Ok(payload) = serde_json::to_value(&event) {
                            let _ = tauri::Emitter::emit(&app_handle, "job-event", payload);
                        }
                    }
                });
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
            backup_agent_identity,
            list_backups,
            restore_from_backup,
            delete_backup,
            disconnect_agent,
            suspend_agent,
            save_recovery_key,
            migrate_legacy_identity,
            check_existing_identity,
            archive_identity,
            wipe_identity,
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
            get_force_override,
            set_force_override,
            diagnose_routing,
            detect_ollama,
            list_recommended_models,
            install_ollama,
            pull_ollama_model,
            list_ollama_models,
            get_ollama_status,
            probe_local_llm,
            set_local_llm_during_birth,
            start_managed_ollama,
            warmup_ollama_model,
            get_config_snapshot,
            set_bundled_model,
            use_stored_provider,
            get_entity_theme,
            get_entity_theme_id,
            set_entity_theme_id,
            get_hive_theme,
            set_hive_theme,
            list_available_themes,
            get_identity_sharing_settings,
            set_identity_sharing_settings,
            get_visual_adaptation_settings,
            set_visual_adaptation_settings,
            get_memory_disclosure_settings,
            set_memory_disclosure_settings,
            get_forge_ui_settings,
            set_forge_advanced_mode,
            get_stored_providers,
            detect_cli_providers,
            detect_cli_providers_full,
            set_active_provider,
            get_active_provider,
            set_routing_mode,
            get_sqlite_stats,
            optimize_sqlite,
            reset_memories,
            search_memories,
            list_sessions,
            get_session_turns,
            recent_memories,
            start_agentic_run,
            start_entity_initiated_agentic_run,
            get_agentic_run_status,
            respond_to_mentor_ask,
            respond_agentic_mentor,
            confirm_tool_execution,
            confirm_agentic_action,
            cancel_agentic_run,
            list_agentic_runs,
            get_agentic_runtime_status,
            get_orchestration_backend_status,
            list_orchestration_jobs,
            set_orchestration_job_enabled,
            delete_orchestration_job,
            run_orchestration_job_now,
            list_orchestration_job_logs,
            list_subagents,
            delegate_to_subagent,
            get_governor_status,
            get_constraint_store,
            clear_constraints,
            upload_chat_attachment,
            get_entity_documents_path,
            save_to_entity_docs,
            get_forge_scenarios,
            crystallize_forge,
            preview_forge_primary_intelligence,
            apply_forge_primary_intelligence,
            forge_undo_last_change,
            get_forge_audit_events,
            get_forge_undo_status,
            genesis_chat,
            chat_stream,
            cancel_chat_stream,
            get_assembled_prompt,
            get_system_diagnostics,
            get_topic_stats,
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
            submit_job,
            list_jobs,
            get_job_status,
            cancel_job,
            list_recurring_templates,
            get_queue_stats,
            get_runtime_mode,
            set_runtime_mode
        ])
        .build(tauri::generate_context!())
        .map_err(|e| format!("Failed to build Tauri app: {e}"))?
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                // Gracefully shut down managed Ollama process
                let state = app_handle.state::<crate::state::AppState>();
                let ollama = state.ollama.clone();
                tauri::async_runtime::block_on(async {
                    let mut guard = ollama.lock().await;
                    if let Some(ref mut mgr) = *guard {
                        tracing::info!("App exiting: shutting down managed Ollama");
                        mgr.shutdown();
                    }
                });
            }
        });

    Ok(())
}
