//! Hive daemon — control plane HTTP server for the Abigail Hive.
//!
//! Wraps `IdentityManager`, `Hive`, and `SecretsVault` behind an Axum REST API.
//! Listens on `--port` (default 43141).

mod birth;
mod bootstrap;
mod doctor;
mod local_auth;
mod persistence;
mod routes;
mod runtime_registry;
mod state;
mod supervisor;

use abigail_core::{AppConfig, SecretsVault};
use abigail_hive::Hive;
use abigail_identity::IdentityManager;
use axum::http::{header, HeaderValue, Method};
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use clap::Parser;

use local_auth::LocalAuth;
use state::HiveDaemonState;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;

/// Upper bound on a single persistence request body (64 MiB).
///
/// Generous rather than tight: this is loopback traffic from the Hive's own
/// supervised children, and the cost of being wrong is a family's memory write
/// failing.
const PERSISTENCE_BODY_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Parser)]
#[command(name = "hive-daemon", about = "Abigail Hive control plane daemon")]
struct Cli {
    /// Port to listen on
    #[arg(long, default_value = "43141")]
    port: u16,

    /// Data directory (defaults to platform-specific app data dir)
    #[arg(long)]
    data_dir: Option<String>,

    /// Run each startup step non-fatally and print [OK]/[FAIL] per step,
    /// then exit instead of serving. For diagnosing a silent first-run
    /// failure from a console.
    #[arg(long)]
    doctor: bool,
}

#[tokio::main]
async fn main() {
    // Durable file logging + panic hook FIRST, before any fallible work. The GUI
    // shell spawns us with no console (invalid inherited stdout/stderr), so any
    // earlier stdout write could kill us silently. See `abigail_diag`.
    abigail_diag::init("hive-daemon");

    let cli = Cli::parse();
    if cli.doctor {
        std::process::exit(doctor::run(cli.data_dir.as_deref(), cli.port));
    }

    if let Err(e) = run().await {
        tracing::error!("hive-daemon exited with error: {e:#}");
        abigail_diag::record_fatal("hive-daemon", &format!("{e:#}"));
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Resolve data directory
    let data_root = if let Some(dir) = &cli.data_dir {
        std::path::PathBuf::from(dir)
    } else {
        AppConfig::default_paths().data_dir
    };
    abigail_core::vault::unlock::configure_process_vault_data_dir(&data_root);

    tracing::info!("Hive data root: {}", data_root.display());

    // Initialize subsystems
    let identity_manager = Arc::new(IdentityManager::new(data_root.clone())?);

    // `SecretsVault::load` is always the right call: it migrates a legacy
    // `secrets.bin` when it finds one and yields an empty vault when there is
    // no file at all. The previous `secrets.bin` existence probe never matched
    // a modern profile (vaults are written as `secrets.vault`), so every
    // restart started from an empty vault and the next save silently wiped the
    // family's stored provider keys.
    let entity_secrets_dir = data_root.join("entity_secrets");
    std::fs::create_dir_all(&entity_secrets_dir)?;
    let entity_secrets = Arc::new(Mutex::new(SecretsVault::load(entity_secrets_dir)?));

    let hive_secrets_dir = data_root.join("hive_secrets");
    std::fs::create_dir_all(&hive_secrets_dir)?;
    let hive_secrets = Arc::new(Mutex::new(SecretsVault::load(hive_secrets_dir)?));

    let hive = Arc::new(Hive::new(entity_secrets.clone(), hive_secrets.clone()));

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", cli.port)).await?;
    let local_addr = listener.local_addr()?;
    let hive_url = format!("http://{}", local_addr);

    // Capture the immortal Hive helper's id before `identity_manager` moves into
    // the daemon state.
    let hive_helper_id = identity_manager
        .hive_agent_id()
        .map_err(anyhow::Error::msg)?;
    let local_auth = LocalAuth::new(std::env::var(hive_core::LOCAL_AUTH_ENV).map_err(|_| {
        anyhow::anyhow!(
            "A caller token must be supplied by the desktop launcher in ABIGAIL_LOCAL_AUTH_TOKEN"
        )
    })?);

    // Take ownership of the shared store before any child is spawned. The
    // embedded SurrealKv file lock is exclusive, so the Hive holds it and
    // serves child daemons over /v1/persistence/op instead of letting each of
    // them try (and fail) to open the same file.
    let persistence = persistence::PersistenceRegistry::open(
        abigail_identity::HiveEntity::memory_db_path(&data_root),
    )?;

    anyhow::ensure!(
        local_auth.token().len() >= 32,
        "Caller token must contain at least 32 characters"
    );
    let supervisor =
        supervisor::HiveSupervisor::new(hive_url.clone(), data_root.clone(), local_auth.clone());

    let active = bootstrap::Bootstrap::saved_connection(
        &*hive_secrets
            .lock()
            .map_err(|_| anyhow::anyhow!("Vault unavailable"))?,
    )?;
    let bootstrap = bootstrap::Bootstrap::open(&data_root, &hive_helper_id, active)?;
    bootstrap.start().await;

    let state = HiveDaemonState {
        identity_manager,
        hive,
        hive_secrets,
        hive_url: hive_url.clone(),
        runtime_control: Arc::new(Mutex::new(runtime_registry::RuntimeControlPlane::default())),
        supervisor: supervisor.clone(),
        local_auth: local_auth.clone(),
        persistence: persistence.clone(),
        bootstrap,
    };

    // Local-only control plane: no wildcard CORS. Browsers on foreign origins
    // cannot invoke privileged routes; authenticated loopback clients still work.
    let cors = CorsLayer::new()
        .allow_origin([
            // Tauri 2 webviews and local Vite dev servers.
            "http://tauri.localhost"
                .parse::<HeaderValue>()
                .expect("valid origin"),
            "http://localhost:1421"
                .parse::<HeaderValue>()
                .expect("valid origin"),
            "http://127.0.0.1:1421"
                .parse::<HeaderValue>()
                .expect("valid origin"),
            "http://localhost:1420"
                .parse::<HeaderValue>()
                .expect("valid origin"),
            "http://127.0.0.1:1420"
                .parse::<HeaderValue>()
                .expect("valid origin"),
            "http://localhost:5173"
                .parse::<HeaderValue>()
                .expect("valid origin"),
            "http://127.0.0.1:5173"
                .parse::<HeaderValue>()
                .expect("valid origin"),
            "tauri://localhost"
                .parse::<HeaderValue>()
                .expect("valid origin"),
            "https://tauri.localhost"
                .parse::<HeaderValue>()
                .expect("valid origin"),
        ])
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT]);

    let app = Router::new()
        .route("/health", get(routes::health))
        .route("/v1/status", get(routes::get_status))
        .route("/v1/setup", get(bootstrap::status))
        .route("/v1/setup/retry", post(bootstrap::retry))
        .route("/v1/setup/cancel", post(bootstrap::cancel))
        .route(
            "/v1/setup/chat",
            get(bootstrap::history).post(bootstrap::chat),
        )
        .route("/v1/setup/activate", post(bootstrap::activate))
        .route("/v1/setup/local", post(bootstrap::use_local))
        .route("/v1/entities", get(routes::list_entities))
        .route("/v1/entities", post(routes::create_entity))
        .route("/v1/entities/:id", get(routes::get_entity))
        .route("/v1/entities/:id/open", post(routes::open_entity))
        .route(
            "/v1/persistence/op",
            post(routes::run_persistence_op)
                // Records that used to be written straight to a local file now
                // travel as a request body, and axum's default cap is 2 MiB.
                // Embeddings, knowledge-base entries and long conversation turns
                // exceed that, and the family would see them silently rejected.
                .layer(axum::extract::DefaultBodyLimit::max(PERSISTENCE_BODY_LIMIT)),
        )
        .route("/v1/entities/:id/close", post(routes::close_entity))
        .route("/v1/birth/scenarios", get(birth::get_scenarios))
        .route(
            "/v1/entities/:id/birth",
            get(birth::get_birth_document).post(birth::perform_birth),
        )
        .route(
            "/v1/entities/:id/config",
            axum::routing::patch(routes::update_entity_config),
        )
        .route(
            "/v1/entities/:id/provider-config",
            get(routes::get_provider_config),
        )
        .route("/v1/entities/:id/sign", post(routes::sign_entity))
        .route(
            "/v1/entities/:id/assignments",
            get(routes::get_skill_assignments).post(routes::set_skill_assignments),
        )
        .route(
            "/v1/entities/:id/forge-approvals",
            get(routes::get_forge_approval_jobs).post(routes::create_forge_approval_job),
        )
        .route("/v1/secrets", post(routes::store_secret))
        .route("/v1/secrets/list", get(routes::list_secrets))
        .route("/v1/secrets/:key", get(routes::get_secret))
        .route("/v1/providers/models", post(routes::discover_models))
        .route("/v1/providers/best", get(routes::get_best_model))
        .route("/v1/providers/detect", get(routes::detect_cli))
        .route("/v1/providers/hive-default", post(routes::set_hive_default))
        .route(
            "/v1/providers/profiles/:name",
            get(routes::get_provider_profile),
        )
        .route("/v1/runtime/sessions", post(routes::issue_runtime_session))
        .route(
            "/v1/runtime/sessions/:lease_id",
            get(routes::get_runtime_session),
        )
        .route("/v1/runtime/register", post(routes::register_runtime))
        .route(
            "/v1/runtime/heartbeat",
            post(routes::record_runtime_heartbeat),
        )
        .route("/v1/runtime/outbox/sync", post(routes::sync_runtime_outbox))
        .route(
            "/v1/entities/:id/execution/receipts",
            get(routes::get_execution_receipts),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            local_auth::require_local_auth,
        ))
        .layer(cors)
        .with_state(state);

    tracing::info!("Hive daemon listening on {}", hive_url);
    // The monolith daemon-manager spawns us with `--port 0` and parses this line
    // from our piped stdout to discover the chosen port. Use a non-panicking
    // write so an invalid/!inherited stdout handle (GUI-shell spawn) can't abort
    // the process the way `println!` would.
    {
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), "Hive daemon listening on {}", hive_url);
    }
    axum::serve(listener, app).await?;

    Ok(())
}
