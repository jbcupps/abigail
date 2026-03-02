//! Entity daemon — agent runtime HTTP server for Abigail.
//!
//! Wraps `IdEgoRouter`, `SkillRegistry`, `SkillExecutor`, and `EventBus` behind
//! an Axum REST API. Fetches provider configuration from hive-daemon on startup.

mod capability_matcher;
mod hive_client;
mod job_scheduler;
mod queue_ops;
mod routes;
mod state;
mod subagent_runner;

use abigail_core::{AppConfig, SecretsVault};
use abigail_hive::Hive;
use abigail_memory::MemoryStore;
use abigail_queue::JobQueue;
use abigail_router::IdEgoRouter;
use abigail_skills::channel::EventBus;
use abigail_skills::skill::SkillConfig;
use abigail_skills::{Skill, SkillExecutionPolicy, SkillExecutor, SkillRegistry};
use abigail_streaming::{IggyBroker, MemoryBroker, StreamBroker, TopicConfig};
use axum::routing::{get, post};
use axum::Router;
use capability_matcher::CapabilityMatcher;
use clap::Parser;
use hive_client::HiveClient;
use job_scheduler::JobScheduler;
use queue_ops::LocalQueueOperations;
use state::EntityDaemonState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use subagent_runner::SubagentRunner;
use tower_http::cors::{Any, CorsLayer};

#[derive(Parser)]
#[command(name = "entity-daemon", about = "Abigail Entity agent runtime daemon")]
struct Cli {
    /// Entity UUID (must be registered in Hive)
    #[arg(long)]
    entity_id: String,

    /// Hive daemon URL
    #[arg(long, default_value = "http://127.0.0.1:3141")]
    hive_url: String,

    /// Port to listen on
    #[arg(long, default_value = "3142")]
    port: u16,

    /// Data directory (defaults to platform-specific app data dir).
    /// Must match the Hive's --data-dir for shared identity resolution.
    #[arg(long)]
    data_dir: Option<String>,

    /// Iggy connection string for persistent event streaming.
    /// When omitted, uses an in-process MemoryBroker (no external deps).
    #[arg(long)]
    iggy_connection: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "entity_daemon=info,abigail_router=info,abigail_skills=info".into()
            }),
        )
        .init();

    let cli = Cli::parse();

    tracing::info!(
        "Entity daemon starting: entity_id={}, hive={}",
        cli.entity_id,
        cli.hive_url
    );

    // 1. Fetch provider config from Hive
    let hive_client = HiveClient::new(&cli.hive_url);
    let entity_info = hive_client.get_entity(&cli.entity_id).await?;
    tracing::info!(
        "Entity '{}' (birth_complete={})",
        entity_info.name,
        entity_info.birth_complete
    );

    let provider_config = hive_client.get_provider_config(&cli.entity_id).await?;
    tracing::info!(
        "Provider config: ego={:?}, routing_mode={}",
        provider_config.ego_provider_name,
        provider_config.routing_mode
    );

    // 2. Build providers from the resolved config
    let tier_models = provider_config
        .tier_models_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<abigail_core::TierModels>(json).ok())
        .unwrap_or_else(abigail_core::TierModels::defaults);

    let tier_thresholds = abigail_core::TierThresholds {
        fast_ceiling: provider_config.tier_threshold_fast_ceiling.unwrap_or(35),
        pro_floor: provider_config.tier_threshold_pro_floor.unwrap_or(70),
    };

    let force_override = provider_config
        .force_override_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<abigail_core::ForceOverride>(json).ok())
        .unwrap_or_default();

    let cli_permission_mode = provider_config
        .cli_permission_mode
        .as_deref()
        .and_then(|s| {
            serde_json::from_str::<abigail_core::CliPermissionMode>(&format!("\"{s}\"")).ok()
        })
        .unwrap_or_default();

    let hive_config = abigail_hive::HiveConfig {
        local_llm_base_url: provider_config.local_llm_base_url,
        ego_provider_name: provider_config.ego_provider_name,
        ego_api_key: provider_config.ego_api_key,
        ego_model: provider_config.ego_model,
        routing_mode: parse_routing_mode(&provider_config.routing_mode),
        tier_models,
        tier_thresholds,
        force_override,
        cli_permission_mode,
    };

    let built = Hive::build_providers(&hive_config).await;

    // 3. Build the router from pre-built providers
    let router = IdEgoRouter::from_built_providers(built);
    let router = Arc::new(router);
    tracing::info!("Router built: {:?}", router.status());

    // 3b. Background model discovery (non-blocking diagnostic)
    {
        let ego_provider = hive_config.ego_provider_name.clone();
        let ego_key = hive_config.ego_api_key.clone();
        tokio::spawn(async move {
            if let (Some(provider), Some(key)) = (ego_provider, ego_key) {
                match abigail_capabilities::cognitive::validation::discover_models(&provider, &key)
                    .await
                {
                    Ok(models) => {
                        tracing::info!("Discovered {} model(s) from {}", models.len(), provider);
                        for m in models.iter().take(5) {
                            tracing::info!("  - {}", m.id);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Model discovery failed for {}: {}", provider, e);
                    }
                }
            }
        });
    }

    // 4. Compute per-entity paths: {data_root}/identities/{entity_id}/
    let data_root = if let Some(dir) = &cli.data_dir {
        std::path::PathBuf::from(dir)
    } else {
        AppConfig::default_paths().data_dir
    };
    tracing::info!("Entity data root: {}", data_root.display());
    let entity_dir = data_root.join("identities").join(&cli.entity_id);
    let docs_dir = entity_dir.join("docs");
    let skills_dir = entity_dir.join("skills");

    // Load entity config when present so policy fields are enforced in runtime.
    let config_path = entity_dir.join("config.json");
    let mut config = if config_path.exists() {
        AppConfig::load(&config_path).unwrap_or_else(|e| {
            tracing::warn!(
                "Failed to load entity config at {}: {}. Falling back to defaults.",
                config_path.display(),
                e
            );
            AppConfig::default_paths()
        })
    } else {
        AppConfig::default_paths()
    };
    config.agent_name = Some(entity_info.name.clone());
    config.birth_complete = entity_info.birth_complete;
    config.routing_mode = hive_config.routing_mode;
    config.data_dir = entity_dir.clone();
    config.docs_dir = docs_dir.clone();
    config.db_path = entity_dir.join("abigail_memory.db");
    config.models_dir = entity_dir.join("models");

    // 5. Create skill registry with secrets vault and executor
    let skill_secrets_dir = data_root.join("skill_secrets");
    std::fs::create_dir_all(&skill_secrets_dir)?;
    let skill_vault = if skill_secrets_dir.join("secrets.bin").exists() {
        SecretsVault::load(skill_secrets_dir)?
    } else {
        SecretsVault::new(skill_secrets_dir)
    };
    let skill_vault = Arc::new(Mutex::new(skill_vault));

    let registry = Arc::new(SkillRegistry::with_secrets(skill_vault.clone()));
    if let Err(e) = registry.set_execution_policy(SkillExecutionPolicy::from_app_config(&config)) {
        tracing::error!("Failed to apply entity skill execution policy: {}", e);
    }
    let executor = Arc::new(SkillExecutor::new(registry.clone()));
    let event_bus = Arc::new(EventBus::new(256));

    // 6. Register HiveManagementSkill (built-in)
    let http_hive_ops = Arc::new(hive_client::HttpHiveOps::new(&cli.hive_url));
    let hive_skill = abigail_skills::HiveManagementSkill::new(http_hive_ops);
    let _ = registry.register(
        abigail_skills::manifest::SkillId("builtin.hive_management".to_string()),
        Arc::new(hive_skill),
    );

    // 6b. Register SkillFactory (allows entity to author skills via chat)
    //     Attach registry + secrets so newly created dynamic_api skills are
    //     immediately registered and usable within the same session.
    {
        let factory_skill = abigail_skills::SkillFactory::new(skills_dir.clone())
            .with_registry(registry.clone())
            .with_secrets(skill_vault.clone());
        let _ = registry.register(
            abigail_skills::manifest::SkillId("builtin.skill_factory".to_string()),
            Arc::new(factory_skill),
        );
    }

    // 7. Load preloaded integration skills (GitHub, Slack, Jira)
    {
        let preloaded = abigail_skills::build_preloaded_skills(Some(skill_vault.clone()));
        for skill in preloaded {
            let skill_id = skill.manifest().id.clone();
            if let Err(e) = registry.register(skill_id.clone(), Arc::new(skill)) {
                tracing::warn!("Failed to register preloaded skill {}: {}", skill_id.0, e);
            }
        }
        tracing::info!("Preloaded integration skills registered (secrets resolved at call time)");
    }

    // 8. Discover dynamic API skills from {entity_dir}/skills/*.json
    {
        let dynamic_skills =
            abigail_skills::DynamicApiSkill::discover(&skills_dir, Some(skill_vault.clone()));
        let count = dynamic_skills.len();
        for skill in dynamic_skills {
            let skill_id = skill.manifest().id.clone();
            if let Err(e) = registry.register(skill_id.clone(), Arc::new(skill)) {
                tracing::warn!("Failed to register dynamic skill {}: {}", skill_id.0, e);
            }
        }
        if count > 0 {
            tracing::info!(
                "Discovered {} dynamic skill(s) from {:?}",
                count,
                skills_dir
            );
        }
    }

    // 9. Sync skill-relevant secrets from Hive into the local skill vault.
    //    This allows the UAT (and operators) to seed all secrets via the Hive API,
    //    and have them automatically available to skill initialization.
    {
        let skill_keys = [
            "imap_password",
            "imap_user",
            "imap_host",
            "imap_port",
            "imap_tls_mode",
        ];
        for key in &skill_keys {
            if let Ok(Some(value)) = hive_client.get_secret(key).await {
                if let Ok(mut v) = skill_vault.lock() {
                    if v.get_secret(key).is_none() {
                        v.set_secret(key, &value);
                        tracing::info!("Synced skill secret '{}' from Hive", key);
                    }
                }
            }
        }
        if let Ok(v) = skill_vault.lock() {
            let _ = v.save();
        }
    }

    // 10. Register and initialize Email (IMAP/SMTP) skill if credentials are available
    {
        let manifest = skill_email::EmailSkill::default_manifest();
        let skill_id = manifest.id.clone();
        let mut skill = skill_email::EmailSkill::new(manifest);

        let has_creds = skill_vault
            .lock()
            .map(|v| v.get_secret("imap_password").is_some())
            .unwrap_or(false);

        if has_creds {
            let (imap_user, imap_password, imap_host, imap_port) = {
                let v = skill_vault.lock().unwrap();
                (
                    v.get_secret("imap_user").unwrap_or("").to_string(),
                    v.get_secret("imap_password").unwrap_or("").to_string(),
                    v.get_secret("imap_host").unwrap_or("").to_string(),
                    v.get_secret("imap_port").unwrap_or("993").to_string(),
                )
            };

            let imap_tls_mode = {
                let v = skill_vault.lock().unwrap();
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
                limits: abigail_skills::sandbox::ResourceLimits::default(),
                permissions: vec![],
                event_sender: Some(Arc::new(event_bus.sender())),
            };

            match tokio::time::timeout(
                std::time::Duration::from_secs(15),
                skill.initialize(skill_config),
            )
            .await
            {
                Ok(Ok(())) => {
                    tracing::info!("Email skill initialized successfully");
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        "Email skill init failed (will register uninitialized): {}",
                        e
                    );
                }
                Err(_) => {
                    tracing::warn!(
                        "Email skill init timed out after 15s (IMAP server unreachable?)"
                    );
                }
            }
        } else {
            tracing::info!(
                "Email skill registered without credentials (no imap_password in vault)"
            );
        }

        if let Err(e) = registry.register(skill_id.clone(), Arc::new(skill)) {
            tracing::warn!("Failed to register Email skill: {}", e);
        }
    }

    // Log total skills loaded
    let total_skills = registry.list().map(|s| s.len()).unwrap_or(0);
    tracing::info!("Total skills registered: {}", total_skills);

    // 9. Open memory store (SQLite, auto-creates schema)
    let memory = Arc::new(
        MemoryStore::open_with_config(&config)
            .expect("Failed to open memory store — check db_path permissions"),
    );
    tracing::info!("Memory store opened: {:?}", config.db_path);

    let instruction_registry = Arc::new({
        let skills_base = data_root.join("skills");
        let reg_path = skills_base.join("registry.toml");
        let instr_dir = skills_base.join("instructions");
        if reg_path.exists() {
            abigail_skills::InstructionRegistry::load(&reg_path, &instr_dir)
        } else {
            abigail_skills::InstructionRegistry::empty()
        }
    });

    let archive_exporter = {
        let pk_path = config.data_dir.join("external_pubkey.bin");
        if pk_path.exists() {
            abigail_memory::ArchiveExporter::with_defaults(pk_path, config.agent_name.as_deref())
                .map(Arc::new)
        } else {
            tracing::info!("No external_pubkey.bin found — archive export disabled");
            None
        }
    };

    // 10. Initialize event streaming (Iggy if configured, otherwise in-process MemoryBroker) and job queue.
    let stream_broker: Arc<dyn StreamBroker> = if let Some(ref conn) = cli.iggy_connection {
        let broker = Arc::new(
            IggyBroker::new(conn.clone())
                .map_err(|e| anyhow::anyhow!("Failed to configure Iggy broker: {}", e))?,
        );
        broker
            .ensure_topic("abigail", "job-events", TopicConfig::default())
            .await
            .map_err(|e| {
                anyhow::anyhow!("Failed to ensure Iggy topic abigail/job-events: {}", e)
            })?;
        broker
            .ensure_consumer_group("abigail", "job-events", "entity-daemon")
            .await
            .map_err(|e| {
                anyhow::anyhow!("Failed to ensure Iggy consumer group entity-daemon: {}", e)
            })?;
        tracing::info!("Connected to Iggy at {}", conn);
        broker
    } else {
        tracing::info!("Using in-process MemoryBroker (no --iggy-connection provided)");
        Arc::new(MemoryBroker::default())
    };

    let queue_conn = rusqlite::Connection::open(&config.db_path).map_err(|e| {
        anyhow::anyhow!(
            "Failed to open queue SQLite DB {}: {}",
            config.db_path.display(),
            e
        )
    })?;
    queue_conn
        .execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| anyhow::anyhow!("Failed to configure queue SQLite WAL mode: {}", e))?;
    queue_conn
        .execute_batch(abigail_queue::MIGRATION_V3_JOB_QUEUE)
        .map_err(|e| anyhow::anyhow!("Failed to apply queue migration: {}", e))?;
    let job_queue = Arc::new(JobQueue::new(
        Arc::new(Mutex::new(queue_conn)),
        stream_broker.clone(),
    ));
    let recovered = job_queue.recover_running_jobs("entity-daemon restarted")?;
    if recovered > 0 {
        tracing::warn!("Recovered {} running jobs at startup", recovered);
    }

    // 11. Register QueueManagementSkill (queue submit/status/list/cancel tools).
    {
        let queue_ops = Arc::new(LocalQueueOperations::new(job_queue.clone()));
        let queue_skill = abigail_skills::QueueManagementSkill::new(queue_ops);
        let _ = registry.register(
            abigail_skills::manifest::SkillId("builtin.queue_management".to_string()),
            Arc::new(queue_skill),
        );
    }

    let state = EntityDaemonState {
        entity_id: cli.entity_id.clone(),
        config,
        router,
        registry,
        executor,
        event_bus,
        docs_dir,
        memory,
        job_queue,
        stream_broker,
        memory_hook: None,
        instruction_registry,
        archive_exporter,
        turns_since_archive: Arc::new(std::sync::atomic::AtomicU32::new(0)),
    };

    // Start background queue scheduler (Phase 1 async sub-agent execution).
    let capability_matcher = CapabilityMatcher::from_router(state.router.clone());
    let subagent_runner = Arc::new(SubagentRunner::new(
        state.job_queue.clone(),
        state.router.clone(),
        state.registry.clone(),
        state.executor.clone(),
        capability_matcher,
        state.config.agent_name.clone(),
    ));
    let scheduler = Arc::new(
        JobScheduler::new(state.job_queue.clone(), subagent_runner)
            .with_max_concurrency(2)
            .with_poll_interval(std::time::Duration::from_millis(500)),
    );
    scheduler.spawn();
    tracing::info!("Job scheduler started (max_concurrency=2)");

    // Spawn SkillsWatcher for hot-reload of new skills (before state is consumed)
    let _watcher = {
        let watch_dir = skills_dir.clone();
        let registry_for_watcher = state.registry.clone();
        let vault_for_watcher = Some(skill_vault.clone());

        match abigail_skills::SkillsWatcher::start(vec![watch_dir]) {
            Ok((watcher, mut rx)) => {
                tokio::spawn(async move {
                    while let Ok(event) = rx.recv().await {
                        match event {
                            abigail_skills::SkillFileEvent::Changed(path) => {
                                tracing::info!("Skill watcher: detected change at {:?}", path);
                                // Check for a sibling JSON file (DynamicApiSkill)
                                if let Some(parent) = path.parent() {
                                    for entry in
                                        std::fs::read_dir(parent).into_iter().flatten().flatten()
                                    {
                                        let p = entry.path();
                                        if p.extension().and_then(|e| e.to_str()) == Some("json") {
                                            match abigail_skills::DynamicApiSkill::load_from_path(
                                                &p,
                                                vault_for_watcher.clone(),
                                            ) {
                                                Ok(skill) => {
                                                    let sid = abigail_skills::manifest::SkillId(
                                                        skill.manifest().id.0.clone(),
                                                    );
                                                    let _ = registry_for_watcher
                                                        .register(sid.clone(), Arc::new(skill));
                                                    tracing::info!(
                                                        "Skill watcher: registered {}",
                                                        sid.0
                                                    );
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
                                tracing::info!("Skill watcher: skill removed at {:?}", path);
                            }
                        }
                    }
                });
                Some(watcher)
            }
            Err(e) => {
                tracing::warn!("Failed to start skills watcher: {}", e);
                None
            }
        }
    };

    // Build HTTP router
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(routes::health))
        .route("/v1/status", get(routes::get_status))
        .route("/v1/chat", post(routes::chat))
        .route("/v1/chat/stream", post(routes::chat_stream))
        .route("/v1/jobs/submit", post(routes::submit_job))
        .route("/v1/jobs", get(routes::list_jobs))
        .route("/v1/jobs/:job_id", get(routes::get_job_status))
        .route("/v1/jobs/:job_id/cancel", post(routes::cancel_job))
        .route("/v1/topics/:topic/results", get(routes::topic_results))
        .route("/v1/topics/:topic/watch", get(routes::watch_topic))
        .route("/v1/routing/diagnose", get(routes::diagnose_routing))
        .route("/v1/skills", get(routes::list_skills))
        .route("/v1/tools/execute", post(routes::execute_tool))
        .route("/v1/memory/stats", get(routes::memory_stats))
        .route("/v1/memory/search", post(routes::memory_search))
        .route("/v1/memory/recent", get(routes::memory_recent))
        .route("/v1/memory/insert", post(routes::memory_insert))
        .layer(cors)
        .with_state(state);

    let addr = format!("127.0.0.1:{}", cli.port);
    tracing::info!("Entity daemon listening on http://{}", addr);
    println!("Entity daemon listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn parse_routing_mode(s: &str) -> abigail_core::RoutingMode {
    match s {
        "EgoPrimary" => abigail_core::RoutingMode::EgoPrimary,
        "Council" => abigail_core::RoutingMode::Council,
        "CliOrchestrator" => abigail_core::RoutingMode::CliOrchestrator,
        // Legacy compatibility shim: "IdPrimary" maps to TierBased.
        // Planned removal window: after 2026-03-31 cleanup review.
        "TierBased" | "IdPrimary" => abigail_core::RoutingMode::TierBased,
        _ => abigail_core::RoutingMode::default(),
    }
}
