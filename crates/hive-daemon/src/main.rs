//! Hive daemon — control plane HTTP server for the Abigail Hive.
//!
//! Wraps `IdentityManager`, `Hive`, and `SecretsVault` behind an Axum REST API.
//! Listens on `--port` (default 3141).

mod routes;
mod state;

use abigail_core::{AppConfig, SecretsVault};
use abigail_hive::Hive;
use abigail_identity::IdentityManager;
use axum::routing::{get, post};
use axum::Router;
use clap::Parser;
use state::HiveDaemonState;
use std::sync::{Arc, Mutex};
use tower_http::cors::{Any, CorsLayer};

#[derive(Parser)]
#[command(name = "hive-daemon", about = "Abigail Hive control plane daemon")]
struct Cli {
    /// Port to listen on
    #[arg(long, default_value = "3141")]
    port: u16,

    /// Data directory (defaults to platform-specific app data dir)
    #[arg(long)]
    data_dir: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "hive_daemon=info,abigail_identity=info,abigail_hive=info".into()
            }),
        )
        .init();

    let cli = Cli::parse();

    // Resolve data directory
    let data_root = if let Some(dir) = &cli.data_dir {
        std::path::PathBuf::from(dir)
    } else {
        AppConfig::default_paths().data_dir
    };

    tracing::info!("Hive data root: {}", data_root.display());

    // Initialize subsystems
    let identity_manager = Arc::new(IdentityManager::new(data_root.clone())?);

    let entity_secrets_dir = data_root.join("entity_secrets");
    std::fs::create_dir_all(&entity_secrets_dir)?;
    let entity_secrets = if entity_secrets_dir.join("secrets.bin").exists() {
        SecretsVault::load(entity_secrets_dir)?
    } else {
        SecretsVault::new(entity_secrets_dir)
    };
    let entity_secrets = Arc::new(Mutex::new(entity_secrets));

    let hive_secrets_dir = data_root.join("hive_secrets");
    std::fs::create_dir_all(&hive_secrets_dir)?;
    let hive_secrets = if hive_secrets_dir.join("secrets.bin").exists() {
        SecretsVault::load(hive_secrets_dir)?
    } else {
        SecretsVault::new(hive_secrets_dir)
    };
    let hive_secrets = Arc::new(Mutex::new(hive_secrets));

    let hive = Arc::new(Hive::new(entity_secrets.clone(), hive_secrets.clone()));

    let state = HiveDaemonState {
        identity_manager,
        hive,
        hive_secrets,
    };

    // Build router
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(routes::health))
        .route("/v1/status", get(routes::get_status))
        .route("/v1/entities", get(routes::list_entities))
        .route("/v1/entities", post(routes::create_entity))
        .route("/v1/entities/:id", get(routes::get_entity))
        .route(
            "/v1/entities/:id/provider-config",
            get(routes::get_provider_config),
        )
        .route("/v1/entities/:id/sign", post(routes::sign_entity))
        .route("/v1/secrets", post(routes::store_secret))
        .route("/v1/secrets/list", get(routes::list_secrets))
        .route("/v1/secrets/:key", get(routes::get_secret))
        .route("/v1/providers/models", post(routes::discover_models))
        .layer(cors)
        .with_state(state);

    let addr = format!("127.0.0.1:{}", cli.port);
    tracing::info!("Hive daemon listening on http://{}", addr);
    println!("Hive daemon listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
