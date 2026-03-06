//! Entity daemon — agent runtime HTTP server for Abigail.
//!
//! Wraps `IdEgoRouter`, `SkillRegistry`, `SkillExecutor`, and `StreamBroker` behind
//! an Axum REST API. Fetches provider configuration from hive-daemon on startup.

mod backup_ops;
mod capability_matcher;
mod hive_client;
mod job_scheduler;
mod memory_consumer;
mod queue_ops;
mod routes;
mod state;
mod subagent_runner;

use abigail_core::{AppConfig, SecretsVault};
use abigail_hive::Hive;
use abigail_identity::IdentityManager;
use abigail_memory::MemoryStore;
use abigail_queue::JobQueue;
use abigail_router::IdEgoRouter;
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
    config.routing_mode = hive_config.routing_mode;
    config.data_dir = entity_dir.clone();
    config.docs_dir = docs_dir.clone();
    config.db_path = entity_dir.join("abigail_memory.db");
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
    let executor = Arc::new(SkillExecutor::new(registry.clone()));

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

    // 8b. Register native Rust skills (matching Tauri in-process registration).
    {
        let mut allowed_roots = vec![entity_dir.clone()];
        allowed_roots.push(std::env::temp_dir());
        if let Some(docs_dir) =
            directories::UserDirs::new().and_then(|u| u.document_dir().map(|d| d.to_path_buf()))
        {
            allowed_roots.push(docs_dir.join("Abigail"));
        }

        macro_rules! register_skill {
            ($skill:expr) => {{
                let s = $skill;
                let id = s.manifest().id.clone();
                if let Err(e) = registry.register(id.clone(), Arc::new(s)) {
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
            entity_dir.clone()
        ));
        register_skill!(skill_knowledge_base::KnowledgeBaseSkill::new(
            skill_knowledge_base::KnowledgeBaseSkill::default_manifest(),
            entity_dir.clone()
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
            skill_vault.clone()
        ));
        register_skill!(
            skill_perplexity_search::PerplexitySearchSkill::with_secrets(
                skill_perplexity_search::PerplexitySearchSkill::default_manifest(),
                skill_vault.clone()
            )
        );

        tracing::info!("Native skills registered for entity-daemon");
    }

    // 9. Sync all declared skill secrets from Hive into the local skill vault.
    //    Collects secret names from: registered skills, discovered manifests,
    //    preloaded integrations, and reserved provider keys.
    {
        let mut all_secret_keys: Vec<String> = Vec::new();

        if let Ok(manifests) = registry.list() {
            for m in &manifests {
                for s in &m.secrets {
                    all_secret_keys.push(s.name.clone());
                }
            }
        }

        let discovered = abigail_skills::SkillRegistry::discover(std::slice::from_ref(&skills_dir));
        for m in &discovered {
            for s in &m.secrets {
                all_secret_keys.push(s.name.clone());
            }
        }

        all_secret_keys.extend(abigail_skills::preloaded_secret_keys());

        for key in abigail_core::RESERVED_PROVIDER_KEYS {
            all_secret_keys.push(key.to_string());
        }

        all_secret_keys.sort();
        all_secret_keys.dedup();

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

    // 9b. Open memory store (SQLite, auto-creates schema)
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

    // 10. Initialize event streaming (Iggy if configured, otherwise in-process MemoryBroker) and job queue.
    let stream_broker: Arc<dyn StreamBroker> = if let Some(ref conn) = cli.iggy_connection {
        let broker = Arc::new(
            IggyBroker::new(conn.clone())
                .map_err(|e| anyhow::anyhow!("Failed to configure Iggy broker: {}", e))?,
        );

        let topics: &[(&str, &str)] = &[
            ("abigail", "job-events"),
            ("abigail", "conversation-turns"),
            ("abigail", "skill-events"),
            ("entity", "conscience-check"),
            ("entity", "ethical-signals"),
        ];
        for (stream, topic) in topics {
            broker
                .ensure_topic(stream, topic, TopicConfig::default())
                .await
                .map_err(|e| {
                    anyhow::anyhow!("Failed to ensure Iggy topic {}/{}: {}", stream, topic, e)
                })?;
            broker
                .ensure_consumer_group(stream, topic, "entity-daemon")
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to ensure Iggy consumer group entity-daemon for {}/{}: {}",
                        stream,
                        topic,
                        e
                    )
                })?;
        }
        tracing::info!(
            "Connected to Iggy at {} ({} topics bootstrapped)",
            conn,
            topics.len()
        );
        broker
    } else {
        tracing::info!("Using in-process MemoryBroker (no --iggy-connection provided)");
        Arc::new(MemoryBroker::default())
    };

    // Phase 5B: Safe async boot - never block the main runtime.
    abigail_skills::set_skill_topology_broker(stream_broker.clone());
    tokio::spawn(async {
        abigail_skills::provision_all_skills("skills/registry.toml").await;
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
        abigail_router::monitors::mentor_chat::start_mentor_chat_monitor(stream_broker.clone())
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
        .map_err(|e| anyhow::anyhow!("Failed to apply queue V3 migration: {}", e))?;
    // V4 orchestration columns (cron, significance, etc.) — idempotent ALTER TABLE ADD COLUMN.
    for stmt in abigail_queue::MIGRATION_V4_ORCHESTRATION.split(';') {
        let trimmed = stmt.trim();
        if !trimmed.is_empty() {
            queue_conn.execute_batch(trimmed).unwrap_or_else(|e| {
                tracing::debug!(
                    "V4 migration statement skipped (likely already applied): {}",
                    e
                );
            });
        }
    }
    // V5 depends_on column for job dependency chains — idempotent ALTER TABLE ADD COLUMN.
    for stmt in abigail_queue::MIGRATION_V5_DEPENDS_ON.split(';') {
        let trimmed = stmt.trim();
        if !trimmed.is_empty() {
            queue_conn.execute_batch(trimmed).unwrap_or_else(|e| {
                tracing::debug!(
                    "V5 migration statement skipped (likely already applied): {}",
                    e
                );
            });
        }
    }
    for stmt in abigail_queue::MIGRATION_V6_EXECUTION_MODE.split(';') {
        let trimmed = stmt.trim();
        if !trimmed.is_empty() {
            queue_conn.execute_batch(trimmed).unwrap_or_else(|e| {
                tracing::debug!(
                    "V6 migration statement skipped (likely already applied): {}",
                    e
                );
            });
        }
    }
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

    // 12. Register and initialize Email (IMAP/SMTP) skill if credentials are available.
    //     Must come after stream_broker creation so the skill can publish events.
    {
        let manifest = skill_email::EmailSkill::default_manifest();
        let skill_id = manifest.id.clone();
        let mut skill = skill_email::EmailSkill::new(manifest);

        let (
            imap_user,
            imap_password,
            imap_host,
            imap_port,
            imap_tls_mode,
            smtp_host,
            smtp_port,
            smtp_user,
            smtp_password,
            smtp_tls_mode,
        ) = tokio::task::spawn_blocking({
            let skill_vault = skill_vault.clone();
            move || {
                if let Ok(v) = skill_vault.lock() {
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
                        v.get_secret("smtp_user").unwrap_or("").to_string(),
                        v.get_secret("smtp_password").unwrap_or("").to_string(),
                        v.get_secret("smtp_tls_mode")
                            .unwrap_or("STARTTLS")
                            .to_string(),
                    )
                } else {
                    (
                        String::new(),
                        String::new(),
                        String::new(),
                        "993".to_string(),
                        "IMPLICIT".to_string(),
                        String::new(),
                        "587".to_string(),
                        String::new(),
                        String::new(),
                        "STARTTLS".to_string(),
                    )
                }
            }
        })
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Failed to join vault read task for email skill: {}", e);
            (
                String::new(),
                String::new(),
                String::new(),
                "993".to_string(),
                "IMPLICIT".to_string(),
                String::new(),
                "587".to_string(),
                String::new(),
                String::new(),
                "STARTTLS".to_string(),
            )
        });

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
        values.insert(
            "smtp_host".to_string(),
            serde_json::Value::String(smtp_host),
        );
        values.insert(
            "smtp_port".to_string(),
            serde_json::json!(smtp_port.parse::<u64>().unwrap_or(587)),
        );
        values.insert(
            "smtp_user".to_string(),
            serde_json::Value::String(smtp_user),
        );
        values.insert(
            "smtp_tls_mode".to_string(),
            serde_json::Value::String(smtp_tls_mode),
        );

        let mut secrets = HashMap::new();
        secrets.insert("imap_password".to_string(), imap_password);
        secrets.insert("smtp_password".to_string(), smtp_password);

        let skill_config = SkillConfig {
            values,
            secrets,
            limits: abigail_skills::sandbox::ResourceLimits::default(),
            permissions: vec![],
            stream_broker: Some(stream_broker.clone()),
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
                tracing::warn!("Email skill init timed out after 15s (IMAP server unreachable?)");
            }
        }

        if let Err(e) = registry.register(skill_id.clone(), Arc::new(skill)) {
            tracing::warn!("Failed to register Email skill: {}", e);
        }
    }

    // Log total skills loaded
    let total_skills = registry.list().map(|s| s.len()).unwrap_or(0);
    tracing::info!("Total skills registered: {}", total_skills);

    let state = EntityDaemonState {
        entity_id: cli.entity_id.clone(),
        config,
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
    };

    // Start background queue scheduler (Phase 1 async sub-agent execution).
    let capability_matcher = CapabilityMatcher::from_router(state.router.clone());
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
        .with_instruction_registry(state.instruction_registry.clone()),
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
        "EgoPrimary" | "TierBased" | "IdPrimary" | "Council" => {
            abigail_core::RoutingMode::EgoPrimary
        }
        "CliOrchestrator" => abigail_core::RoutingMode::CliOrchestrator,
        _ => abigail_core::RoutingMode::default(),
    }
}
