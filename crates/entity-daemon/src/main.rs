//! Entity daemon — agent runtime HTTP server for Abigail.
//!
//! Wraps `IdEgoRouter`, `SkillRegistry`, `SkillExecutor`, and `StreamBroker` behind
//! an Axum REST API. Fetches provider configuration from hive-daemon on startup.

mod backup_ops;
mod capability_matcher;
mod execution_ledger;
mod hive_client;
mod job_scheduler;
mod memory_consumer;
mod outbox;
mod pipeline;
mod queue_ops;
mod router_build;
mod routes;
mod state;
mod subagent_runner;

use abigail_core::{AppConfig, SecretsVault};
use abigail_identity::{HiveEntity, IdentityManager};
use abigail_memory::MemoryStore;
use abigail_persistence::{EntityScope, PersistenceHandle};
use abigail_queue::JobQueue;
use abigail_runtime::{
    collect_declared_secret_keys, register_dynamic_api_skills, register_hive_management_skill,
    register_identity_bound_skills, register_preloaded_skills, register_skill_factory,
    register_supported_native_skills,
};
use abigail_skills::{Skill, SkillExecutionPolicy, SkillExecutor, SkillRegistry};
use abigail_streaming::{MemoryBroker, StreamBroker};
use axum::routing::{get, post};
use axum::Router;
use capability_matcher::CapabilityMatcher;
use clap::Parser;
use hive_client::HiveClient;
use job_scheduler::JobScheduler;
use queue_ops::LocalQueueOperations;
use state::EntityDaemonState;
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
    #[arg(long, default_value = "http://127.0.0.1:43141")]
    hive_url: String,

    /// Port to listen on
    #[arg(long, default_value = "43142")]
    port: u16,

    /// Data directory (defaults to platform-specific app data dir).
    /// Must match the Hive's --data-dir for shared identity resolution.
    #[arg(long)]
    data_dir: Option<String>,

    /// Reserved for a future external broker.
    /// The current dev build always uses the in-process MemoryBroker.
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
    let entity_id = uuid::Uuid::parse_str(&cli.entity_id)
        .map(|id| id.to_string())
        .map_err(|e| anyhow::anyhow!("--entity-id must be a valid UUID: {}", e))?;

    // 1. Fetch provider config from Hive
    let hive_client = HiveClient::new(&cli.hive_url);
    let entity_info = hive_client.get_entity(&entity_id).await?;
    tracing::info!(
        "Entity '{}' (birth_complete={})",
        entity_info.name,
        entity_info.birth_complete
    );

    // Idempotent birth re-read: the full runtime identity in one shot.
    // Hive-side changes (provider, certificate, assignments) apply on the
    // next launch with no re-provisioning. Falls back to the older
    // provider-config endpoint for pre-birth-document hives.
    let provider_config = match hive_client.get_birth_document(&entity_id).await {
        Ok(birth_doc) => {
            if let Some(ref certificate) = birth_doc.certificate {
                tracing::info!(
                    "Born as {} — {}",
                    certificate.archetype,
                    certificate.epithet
                );
            }
            birth_doc.provider_config
        }
        Err(e) => {
            tracing::debug!("Birth document unavailable ({}); using provider-config", e);
            hive_client.get_provider_config(&entity_id).await?
        }
    };
    tracing::info!(
        "Provider config: ego={:?}, routing_mode={}",
        provider_config.ego_provider_name,
        provider_config.routing_mode
    );

    let session_lease = hive_client
        .issue_runtime_session(&entity_id, Some(format!("entity-runtime-{}", entity_id)))
        .await?;
    tracing::info!(
        "Issued runtime session lease {} for runtime {}",
        session_lease.lease_id,
        session_lease.runtime_id
    );

    // 2/3. Build providers + router from the resolved config (shared with the
    // governance hot-swap path). Keep a JSON snapshot so the supervision loop
    // can detect hive-side provider changes.
    let initial_provider_config = serde_json::to_value(&provider_config).unwrap_or_default();
    let ego_provider_for_discovery = provider_config.ego_provider_name.clone();
    let ego_key_for_discovery = provider_config.ego_api_key.clone();
    let router = Arc::new(router_build::build_router(provider_config).await);
    let router = Arc::new(state::RouterHandle::new(router));
    tracing::info!("Router built");

    // 3b. Background model discovery (non-blocking diagnostic)
    {
        let ego_provider = ego_provider_for_discovery;
        let ego_key = ego_key_for_discovery;
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
    abigail_core::vault::unlock::configure_process_vault_data_dir(&data_root);
    tracing::info!("Entity data root: {}", data_root.display());
    let entity_dir = data_root.join("identities").join(&entity_id);
    let docs_dir = entity_dir.join("docs");
    let skills_dir = entity_dir.join("skills");
    let shared_skills_base = data_root.join("skills");
    let shared_registry_path = shared_skills_base.join("registry.toml");

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
    config.is_hive = entity_info.is_hive;
    config.routing_mode = router.current().mode;
    config.data_dir = entity_dir.clone();
    config.docs_dir = docs_dir.clone();
    config.db_path = HiveEntity::memory_db_path(&data_root);
    config.models_dir = entity_dir.join("models");

    // 5. Create skill registry with secrets vault and executor.
    //    Vault initialization touches disk and keychain, so keep it off the async scheduler.
    let skill_secrets_dir = data_root.join("skill_secrets");
    let skill_vault = tokio::task::spawn_blocking({
        let skill_secrets_dir = skill_secrets_dir.clone();
        move || -> anyhow::Result<SecretsVault> {
            std::fs::create_dir_all(&skill_secrets_dir)?;
            if skill_secrets_dir.join("secrets.vault").exists()
                || skill_secrets_dir.join("secrets.bin").exists()
            {
                Ok(SecretsVault::load(skill_secrets_dir)?)
            } else {
                Ok(SecretsVault::new(skill_secrets_dir))
            }
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!("Failed to join vault init task: {}", e))??;
    let skill_vault = Arc::new(Mutex::new(skill_vault));

    let registry = Arc::new(SkillRegistry::with_secrets(skill_vault.clone()));
    if let Err(e) = registry.set_execution_policy(SkillExecutionPolicy::from_app_config(&config)) {
        tracing::error!("Failed to apply entity skill execution policy: {}", e);
    }

    // In-process stream broker — created before the executor so every tool
    // execution publishes a `Topic::SkillExecuted` audit envelope.
    if let Some(ref conn) = cli.iggy_connection {
        tracing::warn!(
            "Ignoring --iggy-connection={} in the current dev build; using in-process MemoryBroker",
            conn
        );
    } else {
        tracing::info!("Using in-process MemoryBroker");
    }
    let stream_broker: Arc<dyn StreamBroker> = Arc::new(MemoryBroker::default());

    let executor =
        Arc::new(SkillExecutor::new(registry.clone()).with_broker(stream_broker.clone()));

    // 5b. Register configured MCP servers as dynamic skills (HTTP transport).
    //     Runs async so a slow/unreachable MCP server never delays boot.
    {
        let mcp_servers: Vec<_> = config
            .mcp_servers
            .iter()
            .filter(|s| s.transport.eq_ignore_ascii_case("http"))
            .cloned()
            .collect();
        if !mcp_servers.is_empty() {
            let trust_policy = config.mcp_trust_policy.clone();
            let registry_for_mcp = registry.clone();
            let broker_for_mcp = stream_broker.clone();
            tokio::spawn(async move {
                for server in mcp_servers {
                    if let Err(policy_err) =
                        trust_policy.validate_http_server_url(&server.id, &server.command_or_url)
                    {
                        tracing::warn!(
                            "MCP server '{}' rejected by trust policy: {}",
                            server.id,
                            policy_err
                        );
                        continue;
                    }
                    let mut runtime = abigail_skills::protocol::mcp::McpSkillRuntime::new(
                        format!("mcp.{}", server.id),
                        format!("MCP {}", server.name),
                        server.command_or_url.clone(),
                        Some(trust_policy.clone()),
                    );
                    if let Err(e) = abigail_skills::Skill::initialize(
                        &mut runtime,
                        abigail_skills::skill::SkillConfig {
                            values: std::collections::HashMap::new(),
                            secrets: std::collections::HashMap::new(),
                            limits: Default::default(),
                            permissions: vec![],
                            stream_broker: Some(broker_for_mcp.clone()),
                        },
                    )
                    .await
                    {
                        tracing::warn!(
                            "MCP server '{}' initialization failed ({}); skipping",
                            server.id,
                            e
                        );
                        continue;
                    }
                    let skill_id = abigail_skills::Skill::manifest(&runtime).id.clone();
                    let tool_count = abigail_skills::Skill::tools(&runtime).len();
                    match registry_for_mcp.register(skill_id.clone(), Arc::new(runtime)) {
                        Ok(_) => tracing::info!(
                            "Registered MCP server '{}' as skill {} ({} tools)",
                            server.id,
                            skill_id.0,
                            tool_count
                        ),
                        Err(e) => {
                            tracing::warn!("Failed to register MCP skill {}: {}", skill_id.0, e)
                        }
                    }
                }
            });
        }
    }

    // 6. Register HiveManagementSkill (built-in)
    let http_hive_ops = Arc::new(hive_client::HttpHiveOps::new(&cli.hive_url));
    register_hive_management_skill(&registry, http_hive_ops);

    // 6b. Register SkillFactory (allows entity to author skills via chat)
    //     Attach registry + secrets so newly created dynamic_api skills are
    //     immediately registered and usable within the same session.
    register_skill_factory(&registry, skills_dir.clone(), skill_vault.clone());

    // 7. Load preloaded integration skills (GitHub, Slack, Jira)
    {
        register_preloaded_skills(&registry, skill_vault.clone());
        tracing::info!("Preloaded integration skills registered (secrets resolved at call time)");
    }

    // 8. Discover dynamic API skills from {entity_dir}/skills/*.json
    {
        let count = register_dynamic_api_skills(&registry, &skills_dir, skill_vault.clone());
        if count > 0 {
            tracing::info!(
                "Discovered {} dynamic skill(s) from {:?}",
                count,
                skills_dir
            );
        }
    }

    // 8b. Register native Rust skills (matching Tauri in-process registration).
    {
        let allow_local_network = config.autonomy_profile.allows_local_network_access();
        register_identity_bound_skills(
            &registry,
            config.data_dir.clone(),
            Some(entity_id.clone()),
            allow_local_network,
        );
        register_supported_native_skills(
            &registry,
            &entity_dir,
            allow_local_network,
            skill_vault.clone(),
        );

        tracing::info!("Native skills registered for entity-daemon");
    }

    // 9. Sync all declared skill secrets from Hive into the local skill vault.
    //    Collects secret names from: registered skills, discovered manifests,
    //    preloaded integrations, and reserved provider keys.
    {
        let all_secret_keys: Vec<String> =
            collect_declared_secret_keys(&registry, std::slice::from_ref(&skills_dir))
                .map(|keys| keys.into_iter().collect())
                .map_err(|e| anyhow::anyhow!("Failed to collect declared secret keys: {}", e))?;

        let mut synced_count = 0u32;
        for key in &all_secret_keys {
            if let Ok(Some(value)) = hive_client.get_secret(key).await {
                let key_owned = key.clone();
                let inserted = tokio::task::spawn_blocking({
                    let skill_vault = skill_vault.clone();
                    let key_owned = key_owned.clone();
                    let value = value.clone();
                    move || {
                        if let Ok(mut v) = skill_vault.lock() {
                            if v.get_secret(&key_owned).is_none() {
                                v.set_secret(&key_owned, &value);
                                return true;
                            }
                        }
                        false
                    }
                })
                .await
                .map_err(|e| anyhow::anyhow!("Failed to join vault sync task: {}", e))?;
                if inserted {
                    synced_count += 1;
                    tracing::info!("Synced skill secret '{}' from Hive", key_owned);
                }
            }
        }
        tokio::task::spawn_blocking({
            let skill_vault = skill_vault.clone();
            move || -> anyhow::Result<()> {
                if let Ok(v) = skill_vault.lock() {
                    v.save()?;
                }
                Ok(())
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("Failed to join vault save task: {}", e))??;
        if synced_count > 0 {
            tracing::info!(
                "Synced {} skill secret(s) from Hive (checked {} declared keys)",
                synced_count,
                all_secret_keys.len()
            );
        }
    }

    // 9b. Open shared memory store (Surreal-backed, auto-creates schema)
    let memory = Arc::new(
        MemoryStore::open_with_config(&config)
            .expect("Failed to open memory store — check db_path permissions"),
    );
    tracing::info!("Memory store opened: {:?}", config.db_path);

    let instruction_registry = Arc::new({
        let reg_path = shared_registry_path.clone();
        let instr_dir = shared_skills_base.join("instructions");
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

    // 10. Event streaming uses the in-process broker created before the executor.
    // Phase 5B: Safe async boot - never block the main runtime.
    abigail_skills::set_skill_topology_broker(stream_broker.clone());
    let provisioning_registry_path = shared_registry_path.clone();
    tokio::spawn(async move {
        let registry_path = provisioning_registry_path.to_string_lossy().to_string();
        abigail_skills::provision_all_skills(&registry_path).await;
    });

    tokio::spawn(async {
        abigail_core::vault::init_resilient().await;
    });

    // Also wrap identity manager for extra safety.
    tokio::task::spawn_blocking({
        let data_root = data_root.clone();
        move || {
            if let Err(e) = IdentityManager::new(data_root) {
                tracing::error!("IdentityManager init failed: {}", e);
            }
        }
    });

    // 10a. Register mentor chat monitor + passive out-of-band observers.
    let _mentor_chat_monitor_handle =
        abigail_router::monitor::mentor_chat::start_mentor_chat_monitor(stream_broker.clone())
            .await
            .map_err(|e| tracing::warn!("Failed to start mentor chat monitor: {}", e))
            .ok();
    let _superego_monitor_handle = abigail_superego::monitor::start(stream_broker.clone())
        .await
        .map_err(|e| tracing::warn!("Failed to start superego monitor: {}", e))
        .ok();
    let _id_monitor_handle = abigail_id::monitor::start(stream_broker.clone())
        .await
        .map_err(|e| tracing::warn!("Failed to start id monitor: {}", e))
        .ok();
    let _devops_forge_worker_handle = soul_forge::worker::spawn_persistent_worker(
        stream_broker.clone(),
        shared_skills_base.clone(),
    )
    .await
    .map_err(|e| tracing::warn!("Failed to start DevOps forge worker: {}", e))
    .ok();
    let _memory_chat_topic_handle =
        abigail_memory::subscriber::start(stream_broker.clone(), memory.clone())
            .await
            .map_err(|e| tracing::warn!("Failed to start memory chat-topic subscriber: {}", e))
            .ok();

    let queue_store = if abigail_persistence::ci_mode_enabled()
        && std::env::var_os("ABIGAIL_DAEMON_INTEGRATION").is_some()
    {
        tracing::info!("Using ephemeral Hive queue store for daemon integration tests");
        PersistenceHandle::open_ephemeral(EntityScope::Hive)
    } else {
        PersistenceHandle::open(HiveEntity::memory_db_path(&data_root), EntityScope::Hive)
    }
    .map_err(|e| anyhow::anyhow!("Failed to open Hive queue store: {}", e))?;
    let job_queue = Arc::new(JobQueue::new(queue_store, stream_broker.clone()));
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

    // 11b. Register BackupManagementSkill (backup list/preview/import tools).
    {
        let backup_ops = Arc::new(backup_ops::LocalBackupOps::new(
            memory.clone(),
            data_root.clone(),
            config.agent_name.clone(),
        ));
        let backup_skill = abigail_skills::BackupManagementSkill::new(backup_ops);
        let _ = registry.register(
            abigail_skills::manifest::SkillId("builtin.backup_management".to_string()),
            Arc::new(backup_skill),
        );
    }

    // Log total skills loaded
    let total_skills = registry.list().map(|s| s.len()).unwrap_or(0);
    tracing::info!("Total skills registered: {}", total_skills);

    let skill_assignments = hive_client
        .get_skill_assignments(&entity_id)
        .await
        .map(|response| response.assignments)
        .unwrap_or_default();
    let forge_jobs = hive_client
        .get_forge_approval_jobs(&entity_id)
        .await
        .map(|response| response.jobs)
        .unwrap_or_default();
    let outbox = Arc::new(outbox::RuntimeOutbox::load(&entity_dir, 256)?);
    let execution_ledger = Arc::new(execution_ledger::ExecutionLedger::load(&entity_dir)?);

    // Soul reference: hash of the entity's soul documents, stamped on every
    // pipeline envelope so downstream observers know which identity version
    // produced a message.
    let soul_ref = {
        let mut soul_bytes = std::fs::read(docs_dir.join("soul.md")).unwrap_or_default();
        soul_bytes
            .extend_from_slice(&std::fs::read(docs_dir.join("ethics.md")).unwrap_or_default());
        abigail_streaming::compute_soul_ref(&soul_bytes)
    };

    let state = EntityDaemonState {
        entity_id: entity_id.clone(),
        config,
        hive_url: cli.hive_url.clone(),
        runtime_id: session_lease.runtime_id.clone(),
        session_lease,
        router,
        registry,
        executor,
        docs_dir,
        memory,
        job_queue,
        stream_broker,
        memory_hook: None,
        instruction_registry,
        archive_exporter,
        turns_since_archive: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        active_stream_cancel: Arc::new(tokio::sync::Mutex::new(None)),
        constraints: Arc::new(tokio::sync::RwLock::new(
            abigail_router::ConstraintStore::with_data_dir(entity_dir.clone()),
        )),
        outbox,
        execution_ledger,
        last_hive_sync_at_utc: Arc::new(tokio::sync::RwLock::new(None)),
        last_hive_error: Arc::new(tokio::sync::RwLock::new(None)),
        runtime_url: Arc::new(tokio::sync::RwLock::new(None)),
        skill_assignments: Arc::new(tokio::sync::RwLock::new(skill_assignments)),
        forge_jobs: Arc::new(tokio::sync::RwLock::new(forge_jobs)),
        recent_skill_acks: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        turns: Arc::new(pipeline::TurnRegistry::default()),
        soul_ref,
    };

    // Spawn the bicameral chat pipeline (Id stage → Ego stage → journal).
    // Fatal on failure: without the pipeline every chat request would hang
    // until the turn timeout, which is worse than refusing to start.
    let _pipeline_handles = pipeline::spawn(state.clone())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start chat pipeline: {}", e))?;

    // Start background queue scheduler (Phase 1 async sub-agent execution).
    let capability_matcher = CapabilityMatcher::from_router(state.router.current());
    let subagent_runner = Arc::new(
        SubagentRunner::new(
            state.job_queue.clone(),
            state.router.clone(),
            state.registry.clone(),
            state.executor.clone(),
            capability_matcher,
            state.config.agent_name.clone(),
        )
        .with_docs_dir(state.docs_dir.clone())
        .with_instruction_registry(state.instruction_registry.clone())
        .with_hive_client(Arc::new(hive_client.clone())),
    );
    let scheduler = Arc::new(
        JobScheduler::new(state.job_queue.clone(), subagent_runner)
            .with_max_concurrency(2)
            .with_poll_interval(std::time::Duration::from_millis(500)),
    );
    scheduler.spawn();
    tracing::info!("Job scheduler started (max_concurrency=2)");

    // Spawn memory consumer — persists conversation turns from StreamBroker topic.
    let _memory_consumer_handle =
        memory_consumer::spawn_memory_consumer(state.stream_broker.clone(), state.memory.clone())
            .await
            .map_err(|e| tracing::warn!("Failed to start memory consumer: {}", e))
            .ok();

    // Spawn conscience consumer — async ethical evaluation via StreamBroker.
    let _conscience_handle = abigail_router::ConscienceConsumer::new(state.stream_broker.clone())
        .spawn()
        .await
        .map_err(|e| tracing::warn!("Failed to start conscience consumer: {}", e))
        .ok();

    // Spawn SkillsWatcher for hot-reload of new/changed/removed skills
    let _watcher = {
        let watch_dir = skills_dir.clone();
        let shared_watch_dir = shared_skills_base.clone();
        let registry_for_watcher = state.registry.clone();
        let vault_for_watcher = Some(skill_vault.clone());
        let broker_for_watcher = state.stream_broker.clone();
        let state_for_watcher = state.clone();

        match abigail_skills::SkillsWatcher::start(vec![watch_dir, shared_watch_dir]) {
            Ok((watcher, mut rx)) => {
                tokio::spawn(async move {
                    while let Ok(event) = rx.recv().await {
                        match event {
                            abigail_skills::SkillFileEvent::Changed(path) => {
                                tracing::info!("Skill watcher: detected change at {:?}", path);
                                let dir = if path.is_file() {
                                    path.parent().map(|p| p.to_path_buf())
                                } else {
                                    Some(path.clone())
                                };
                                if let Some(parent) = dir {
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
                                                    abigail_skills::channel::event::publish_skill_event(
                                                        &broker_for_watcher,
                                                        abigail_skills::channel::event::SkillEvent {
                                                            skill_id: sid,
                                                            trigger: "skill_reloaded".to_string(),
                                                            payload: serde_json::json!({ "path": p.display().to_string() }),
                                                            timestamp: chrono::Utc::now(),
                                                            priority: abigail_skills::channel::TriggerPriority::Normal,
                                                        },
                                                    )
                                                    .await;
                                                    state_for_watcher
                                                        .push_skill_ack(entity_core::SkillApplyAcknowledgement {
                                                            skill_id: p
                                                                .file_stem()
                                                                .and_then(|stem| stem.to_str())
                                                                .unwrap_or("unknown")
                                                                .to_string(),
                                                            status: "reloaded".to_string(),
                                                            applied_at_utc: chrono::Utc::now()
                                                                .to_rfc3339(),
                                                        })
                                                        .await;
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
                                let is_json =
                                    path.extension().and_then(|e| e.to_str()) == Some("json");
                                if is_json {
                                    if let Ok(bytes) = std::fs::read_to_string(&path) {
                                        if let Ok(cfg) =
                                            serde_json::from_str::<
                                                abigail_skills::dynamic::DynamicSkillConfig,
                                            >(&bytes)
                                        {
                                            let sid =
                                                abigail_skills::manifest::SkillId(cfg.id.clone());
                                            let _ = registry_for_watcher.unregister(&sid);
                                            tracing::info!("Skill watcher: unregistered {}", sid.0);
                                        }
                                    }
                                }
                                // For skill.toml removal: scan sibling JSONs and unregister
                                if let Some(parent) = path.parent() {
                                    for entry in
                                        std::fs::read_dir(parent).into_iter().flatten().flatten()
                                    {
                                        let p = entry.path();
                                        if p.extension().and_then(|e| e.to_str()) == Some("json") {
                                            if let Ok(bytes) = std::fs::read_to_string(&p) {
                                                if let Ok(cfg) = serde_json::from_str::<
                                                    abigail_skills::dynamic::DynamicSkillConfig,
                                                >(
                                                    &bytes
                                                ) {
                                                    let sid = abigail_skills::manifest::SkillId(
                                                        cfg.id.clone(),
                                                    );
                                                    let _ = registry_for_watcher.unregister(&sid);
                                                    tracing::info!(
                                                        "Skill watcher: unregistered {}",
                                                        sid.0
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
                                                    )
                                                    .await;
                                                    state_for_watcher
                                                        .push_skill_ack(entity_core::SkillApplyAcknowledgement {
                                                            skill_id: p
                                                                .file_stem()
                                                                .and_then(|stem| stem.to_str())
                                                                .unwrap_or("unknown")
                                                                .to_string(),
                                                            status: "removed".to_string(),
                                                            applied_at_utc: chrono::Utc::now()
                                                                .to_rfc3339(),
                                                        })
                                                        .await;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            abigail_skills::SkillFileEvent::RegistryChanged(path) => {
                                tracing::info!(
                                    "Skill watcher: registry changed at {:?} (topology hot-reload dispatched)",
                                    path
                                );
                            }
                            abigail_skills::SkillFileEvent::RegistryRemoved(path) => {
                                tracing::warn!(
                                    "Skill watcher: registry removed at {:?}; persistent topology cancelled",
                                    path
                                );
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
        .route("/v1/session/status", get(routes::get_session_status))
        .route("/v1/outbox/status", get(routes::get_outbox_status))
        .route("/v1/execution/events", get(routes::get_execution_events))
        .route("/v1/chat", post(routes::chat))
        .route("/v1/chat/stream", post(routes::chat_stream))
        .route("/v1/chat/cancel", post(routes::cancel_chat_stream))
        .route(
            "/v1/governance/constraints",
            get(routes::get_constraints).delete(routes::clear_constraints),
        )
        .route("/v1/governance/status", get(routes::get_governance_status))
        .route("/v1/jobs/submit", post(routes::submit_job))
        .route("/v1/jobs", get(routes::list_jobs))
        .route("/v1/jobs/:job_id", get(routes::get_job_status))
        .route("/v1/jobs/:job_id/cancel", post(routes::cancel_job))
        .route("/v1/topics/:topic/results", get(routes::topic_results))
        .route("/v1/topics/:topic/watch", get(routes::watch_topic))
        .route("/v1/routing/diagnose", get(routes::diagnose_routing))
        .route("/v1/skills", get(routes::list_skills))
        .route(
            "/v1/skills/acks",
            get(routes::list_skill_apply_acknowledgements),
        )
        .route("/v1/tools/execute", post(routes::execute_tool))
        .route("/v1/memory/stats", get(routes::memory_stats))
        .route("/v1/memory/search", post(routes::memory_search))
        .route("/v1/memory/recent", get(routes::memory_recent))
        .route("/v1/memory/insert", post(routes::memory_insert))
        .layer(cors)
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", cli.port)).await?;
    let local_addr = listener.local_addr()?;
    let local_url = format!("http://{}", local_addr);
    {
        let mut runtime_url = state.runtime_url.write().await;
        *runtime_url = Some(local_url.clone());
    }

    hive_client
        .register_runtime(&hive_core::RuntimeRegistrationRequest {
            lease_id: state.session_lease.lease_id.clone(),
            runtime_id: state.runtime_id.clone(),
            local_url: local_url.clone(),
            process_id: Some(std::process::id()),
        })
        .await?;
    state.record_hive_sync_success().await;
    spawn_runtime_supervision(state.clone(), hive_client.clone(), initial_provider_config);

    tracing::info!("Entity daemon listening on {}", local_url);
    println!("Entity daemon listening on {}", local_url);
    axum::serve(listener, app).await?;

    Ok(())
}

fn spawn_runtime_supervision(
    state: EntityDaemonState,
    hive_client: HiveClient,
    initial_provider_config: serde_json::Value,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(10));
        let mut tick_count: u64 = 0;
        let mut last_provider_config = initial_provider_config;
        loop {
            ticker.tick().await;
            tick_count += 1;

            // Every 6th tick (~60s): check the Hive for a changed provider
            // config and publish a governance refresh so the router hot-swaps
            // without an entity-daemon restart.
            if tick_count.is_multiple_of(6) {
                if let Ok(fresh) = hive_client.get_provider_config(&state.entity_id).await {
                    let fresh_json = serde_json::to_value(&fresh).unwrap_or_default();
                    if fresh_json != last_provider_config {
                        tracing::info!(
                            "Provider config changed on hive (ego={:?}); publishing refresh",
                            fresh.ego_provider_name
                        );
                        last_provider_config = fresh_json.clone();
                        let envelope = abigail_streaming::Envelope::new(
                            abigail_streaming::Topic::GovernanceInbound,
                            state.entity_id.clone(),
                            uuid::Uuid::new_v4().to_string(),
                            serde_json::json!({
                                "kind": pipeline::governance::KIND_PROVIDER_REFRESH,
                                "provider_config": fresh_json,
                            }),
                        )
                        .with_soul_ref(state.soul_ref.clone());
                        if let Err(e) = envelope.publish(state.stream_broker.as_ref()).await {
                            tracing::warn!("Failed to publish provider refresh: {}", e);
                        }
                    }
                }
            }

            // Every 3rd tick (~30s): check the Hive for changed skill
            // assignments and publish a governance refresh on the bus so
            // they apply live without an entity-daemon restart.
            if tick_count.is_multiple_of(3) {
                if let Ok(response) = hive_client.get_skill_assignments(&state.entity_id).await {
                    let fresh = response.assignments;
                    let changed = {
                        let current = state.skill_assignments.read().await;
                        serde_json::to_value(&*current).ok() != serde_json::to_value(&fresh).ok()
                    };
                    if changed {
                        let envelope = abigail_streaming::Envelope::new(
                            abigail_streaming::Topic::GovernanceInbound,
                            state.entity_id.clone(),
                            uuid::Uuid::new_v4().to_string(),
                            serde_json::json!({
                                "kind": pipeline::governance::KIND_SKILL_ASSIGNMENTS_REFRESH,
                                "assignments": fresh,
                            }),
                        )
                        .with_soul_ref(state.soul_ref.clone());
                        if let Err(e) = envelope.publish(state.stream_broker.as_ref()).await {
                            tracing::warn!("Failed to publish assignment refresh: {}", e);
                        }
                    }
                }
            }

            let outbox_status = match state.outbox.status() {
                Ok(status) => status,
                Err(error) => {
                    state.record_hive_sync_error(error).await;
                    continue;
                }
            };
            let runtime_url = state.runtime_url.read().await.clone();

            let heartbeat = hive_core::RuntimeHeartbeatRequest {
                lease_id: state.session_lease.lease_id.clone(),
                runtime_id: state.runtime_id.clone(),
                local_url: runtime_url,
                outbox_depth: outbox_status.queued_records,
                outbox_oldest_at_utc: outbox_status.oldest_record_at_utc.clone(),
            };

            match hive_client.heartbeat(&heartbeat).await {
                Ok(_) => {
                    if let Ok(records) = state.outbox.snapshot() {
                        if !records.is_empty() {
                            match hive_client
                                .sync_outbox(&hive_core::OutboxSyncRequest {
                                    lease_id: state.session_lease.lease_id.clone(),
                                    runtime_id: state.runtime_id.clone(),
                                    records,
                                })
                                .await
                            {
                                Ok(response) => {
                                    let _ = state.outbox.acknowledge(&response.accepted_record_ids);
                                }
                                Err(error) => {
                                    let error = error.to_string();
                                    let _ = state.outbox.mark_sync_error(&error);
                                    state.record_hive_sync_error(error).await;
                                    continue;
                                }
                            }
                        }
                    }
                    state.record_hive_sync_success().await;
                }
                Err(error) => {
                    let error = error.to_string();
                    let _ = state.outbox.mark_sync_error(&error);
                    state.record_hive_sync_error(error).await;
                }
            }
        }
    });
}
