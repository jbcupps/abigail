//! Entity daemon — agent runtime HTTP server for Abigail.
//!
//! Wraps `IdEgoRouter`, `SkillRegistry`, `SkillExecutor`, and `EventBus` behind
//! an Axum REST API. Fetches provider configuration from hive-daemon on startup.

mod hive_client;
mod routes;
mod state;

use abigail_core::AppConfig;
use abigail_hive::Hive;
use abigail_memory::MemoryStore;
use abigail_router::IdEgoRouter;
use abigail_skills::channel::EventBus;
use abigail_skills::{Skill, SkillExecutor, SkillRegistry};
use axum::routing::{get, post};
use axum::Router;
use clap::Parser;
use hive_client::HiveClient;
use state::EntityDaemonState;
use std::sync::Arc;
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
    let hive_config = abigail_hive::HiveConfig {
        local_llm_base_url: provider_config.local_llm_base_url,
        ego_provider_name: provider_config.ego_provider_name,
        ego_api_key: provider_config.ego_api_key,
        ego_model: provider_config.ego_model,
        routing_mode: parse_routing_mode(&provider_config.routing_mode),
        superego_provider: provider_config.superego_provider,
        superego_api_key: provider_config.superego_api_key,
        superego_l2_mode: parse_superego_l2_mode(&provider_config.superego_l2_mode),
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
    let data_root = AppConfig::default_paths().data_dir;
    let entity_dir = data_root.join("identities").join(&cli.entity_id);
    let docs_dir = entity_dir.join("docs");
    let skills_dir = entity_dir.join("skills");

    // 5. Create skill registry and executor
    let registry = Arc::new(SkillRegistry::new());
    let executor = Arc::new(SkillExecutor::new(registry.clone()));
    let event_bus = Arc::new(EventBus::new(256));

    // 6. Register HiveManagementSkill (built-in)
    let http_hive_ops = Arc::new(hive_client::HttpHiveOps::new(&cli.hive_url));
    let hive_skill = abigail_skills::HiveManagementSkill::new(http_hive_ops);
    let _ = registry.register(
        abigail_skills::manifest::SkillId("builtin.hive_management".to_string()),
        Arc::new(hive_skill),
    );

    // 7. Load preloaded integration skills (GitHub, Slack, Jira)
    {
        let preloaded = abigail_skills::build_preloaded_skills(None);
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
        let dynamic_skills = abigail_skills::DynamicApiSkill::discover(&skills_dir, None);
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

    let config = AppConfig {
        agent_name: Some(entity_info.name),
        birth_complete: entity_info.birth_complete,
        routing_mode: hive_config.routing_mode,
        data_dir: entity_dir.clone(),
        docs_dir: docs_dir.clone(),
        db_path: entity_dir.join("abigail_memory.db"),
        models_dir: entity_dir.join("models"),
        ..AppConfig::default_paths()
    };

    // Log total skills loaded
    let total_skills = registry.list().map(|s| s.len()).unwrap_or(0);
    tracing::info!("Total skills registered: {}", total_skills);

    // 9. Open memory store (SQLite, auto-creates schema)
    let memory = Arc::new(
        MemoryStore::open_with_config(&config)
            .expect("Failed to open memory store — check db_path permissions"),
    );
    tracing::info!("Memory store opened: {:?}", config.db_path);

    let state = EntityDaemonState {
        entity_id: cli.entity_id.clone(),
        config,
        router,
        registry,
        executor,
        event_bus,
        docs_dir,
        memory,
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
        "IdPrimary" => abigail_core::RoutingMode::IdPrimary,
        "EgoPrimary" => abigail_core::RoutingMode::EgoPrimary,
        "Council" => abigail_core::RoutingMode::Council,
        "TierBased" => abigail_core::RoutingMode::TierBased,
        _ => abigail_core::RoutingMode::default(),
    }
}

fn parse_superego_l2_mode(s: &str) -> abigail_core::SuperegoL2Mode {
    match s {
        "Enforce" => abigail_core::SuperegoL2Mode::Enforce,
        "Advisory" => abigail_core::SuperegoL2Mode::Advisory,
        "Off" => abigail_core::SuperegoL2Mode::Off,
        _ => abigail_core::SuperegoL2Mode::default(),
    }
}
