//! Hive daemon — control plane HTTP server for the Abigail Hive.
//!
//! Wraps `IdentityManager`, `Hive`, and `SecretsVault` behind an Axum REST API.
//! Listens on `--port` (default 43141).

mod birth;
mod doctor;
mod genesis;
mod genesis_auth;
mod genesis_creation;
mod genesis_director;
mod genesis_media;
mod genesis_retention;
mod genesis_store;
mod model_routing;
mod persistence;
mod presence;
mod presence_profile;
mod profile_runtime;
mod routes;
mod runtime_registry;
mod state;
mod supervisor;

use abigail_core::profile::{ProcessIdentity, ProfileContext};
use abigail_core::{AppConfig, SecretsVault};
use abigail_hive::Hive;
use abigail_identity::IdentityManager;
use axum::routing::{get, post};
use axum::Router;
use clap::Parser;
use state::HiveDaemonState;
use std::sync::{Arc, Mutex};
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

#[derive(Parser)]
#[command(name = "hive-daemon", about = "Abigail Hive control plane daemon")]
struct Cli {
    /// Port to listen on
    #[arg(long)]
    port: Option<u16>,

    /// Explicit application profile. Preview must agree with the launcher context.
    #[arg(long)]
    profile: Option<String>,

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
    let cli = Cli::parse();
    // Refusal must precede logs, vault access, store initialization and doctor.
    let prepared = (|| -> anyhow::Result<_> {
        let preview_requested = abigail_core::profile::compiled_preview()
            || cli.profile.as_deref() == Some("preview")
            || std::env::var("ABIGAIL_PROFILE").ok().as_deref() == Some("preview");
        if preview_requested {
            anyhow::ensure!(
                profile_runtime::preview_port_allowed(cli.port.unwrap_or(0)),
                "Preview Hive cannot bind a normal daemon port"
            );
            profile_runtime::validate_launch_arguments(
                cli.profile.as_deref(),
                cli.data_dir.as_deref().map(std::path::Path::new),
            )?;
            anyhow::ensure!(
                std::env::var_os("ABIGAIL_HIVE_URL").is_none(),
                "Preview Hive launch cannot inherit an existing Hive endpoint"
            );
            anyhow::ensure!(
                std::env::var_os("ABIGAIL_CI_MODE").is_none()
                    && std::env::var_os("ABIGAIL_DAEMON_INTEGRATION").is_none(),
                "Preview Hive cannot inherit ephemeral test-mode flags"
            );
            for key in [
                "ABIGAIL_PROFILE_ID",
                "ABIGAIL_PROCESS_INSTANCE_ID",
                "ABIGAIL_HIVE_INSTANCE_ID",
            ] {
                let value = std::env::var(key)
                    .map_err(|_| anyhow::anyhow!("Preview Hive requires {key}"))?;
                abigail_core::profile::validate_uuid(&value)?;
            }
            anyhow::ensure!(
                std::env::var("ABIGAIL_PROCESS_INSTANCE_ID")?
                    == std::env::var("ABIGAIL_HIVE_INSTANCE_ID")?,
                "Preview Hive process and Hive instance identifiers must match"
            );
            supervisor::entity_daemon_binary_for_profile(true)?;
        }
        let profile = abigail_core::profile::initialize_process(
            cli.profile.as_deref(),
            cli.data_dir.as_deref().map(std::path::Path::new),
        )?;
        let identity = profile
            .as_deref()
            .map(|profile| {
                profile_runtime::launch_identity(
                    profile,
                    std::env::var("ABIGAIL_PROFILE_ID").ok().as_deref(),
                    std::env::var("ABIGAIL_PROCESS_INSTANCE_ID").ok().as_deref(),
                    std::env::var("ABIGAIL_HIVE_INSTANCE_ID").ok().as_deref(),
                )
            })
            .transpose()?;
        Ok((profile, identity))
    })();
    let (profile, identity) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            use std::io::Write as _;
            let _ = writeln!(std::io::stderr(), "Hive startup refused: {error:#}");
            std::process::exit(1);
        }
    };
    let _profile_owner = match profile
        .as_deref()
        .map(profile_runtime::ProfileOwner::acquire)
        .transpose()
    {
        Ok(owner) => owner,
        Err(error) => {
            use std::io::Write as _;
            let _ = writeln!(std::io::stderr(), "Hive ownership refused: {error:#}");
            std::process::exit(1);
        }
    };
    if let Some(profile) = profile.as_deref() {
        if let Err(error) = abigail_diag::init_with_log_dir("hive-daemon", &profile.logs_dir()) {
            use std::io::Write as _;
            let _ = writeln!(
                std::io::stderr(),
                "Hive logging initialization failed: {error}"
            );
            std::process::exit(1);
        }
    } else {
        abigail_diag::init("hive-daemon");
    }
    let port = cli
        .port
        .unwrap_or(if profile.is_some() { 0 } else { 43141 });
    if cli.doctor {
        let root = profile
            .as_ref()
            .map(|context| context.root().to_string_lossy().to_string());
        std::process::exit(doctor::run(
            root.as_deref().or(cli.data_dir.as_deref()),
            port,
        ));
    }

    if let Err(e) = run(cli, profile, identity, port).await {
        tracing::error!("hive-daemon exited with error: {e:#}");
        abigail_diag::record_fatal("hive-daemon", &format!("{e:#}"));
        std::process::exit(1);
    }
}

async fn run(
    cli: Cli,
    profile: Option<Arc<ProfileContext>>,
    process_identity: Option<ProcessIdentity>,
    port: u16,
) -> anyhow::Result<()> {
    // Resolve data directory
    let data_root = if let Some(profile) = profile.as_deref() {
        profile.root().to_path_buf()
    } else if let Some(dir) = &cli.data_dir {
        std::path::PathBuf::from(dir)
    } else {
        AppConfig::default_paths().data_dir
    };
    abigail_core::vault::unlock::configure_process_vault_data_dir(&data_root);

    tracing::info!("Hive data root: {}", data_root.display());

    // Initialize subsystems
    let identity_manager = Arc::new(IdentityManager::new(data_root.clone())?);
    // Keep the engine in this process for the lifetime of the Hive. Entity
    // daemons access their scoped databases over HTTP rather than contending
    // for the embedded engine's filesystem lock.
    let _shared_store = abigail_persistence::PersistenceHandle::open(
        abigail_identity::HiveEntity::memory_db_path(&data_root),
        abigail_persistence::EntityScope::Hive,
    )?;

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

    let mut attempts = 0;
    let listener = loop {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
        if profile.is_none() || profile_runtime::preview_port_allowed(listener.local_addr()?.port())
        {
            break listener;
        }
        drop(listener);
        attempts += 1;
        anyhow::ensure!(
            attempts < 16,
            "Could not allocate a separate preview Hive port"
        );
    };
    let local_addr = listener.local_addr()?;
    let hive_url = format!("http://{}", local_addr);

    // Capture the immortal Hive helper's id before `identity_manager` moves into
    // the daemon state.
    let hive_helper_id = identity_manager.hive_agent_id();
    let helper = Arc::new(Mutex::new(None));
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let supervisor = supervisor::HiveSupervisor::with_profile(
        hive_url.clone(),
        data_root.clone(),
        profile.clone(),
        process_identity.clone(),
    );

    let state = HiveDaemonState {
        genesis: Arc::new(genesis_store::GenesisStore::new(_shared_store.clone())?),
        genesis_media: Arc::new(genesis_media::GenesisMediaService::new(
            &data_root,
            hive.clone(),
        )?),
        genesis_token: Arc::new(genesis_auth::load_or_create_token(&data_root)?),
        genesis_control: Arc::new(genesis::GenesisControl::default()),
        identity_manager,
        hive,
        hive_secrets,
        hive_url: hive_url.clone(),
        process_identity: process_identity.clone(),
        shutdown: shutdown.clone(),
        runtime_control: Arc::new(Mutex::new(runtime_registry::RuntimeControlPlane::default())),
        helper: helper.clone(),
        supervisor: supervisor.clone(),
    };

    let _genesis_worker = state.genesis_media.clone().start_worker();
    let _genesis_retention = genesis_retention::start_genesis_retention(state.clone());

    // Start the persistent Hive helper out-of-band: the immortal "Abigail Hive"
    // identity, run as its own entity-daemon. The control plane keeps serving
    // /health immediately while the helper warms up.
    match hive_helper_id {
        Ok(id) => supervisor::supervise_hive_helper(supervisor, id, helper),
        Err(e) => tracing::warn!("Hive helper not started: {}", e),
    }

    // Build router
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            origin
                .to_str()
                .ok()
                .and_then(|s| reqwest::Url::parse(s).ok())
                .is_some_and(|url| {
                    matches!(
                        url.host_str(),
                        Some("localhost" | "127.0.0.1" | "tauri.localhost" | "[::1]" | "::1")
                    )
                })
        }))
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .merge(genesis::router(state.clone()))
        .route("/health", get(routes::health))
        .route("/v1/status", get(routes::get_status))
        .route("/v1/shutdown", post(routes::shutdown))
        .route("/v1/entities", get(routes::list_entities))
        .route("/v1/entities", post(routes::create_entity))
        .route(
            "/v1/entity-creations/:creation_key",
            get(routes::get_entity_creation),
        )
        .route("/v1/entities/:id", get(routes::get_entity))
        .merge(presence::router(state.identity_manager.clone()))
        .route("/v1/entities/:id/open", post(routes::open_entity))
        .route("/v1/entities/:id/close", post(routes::close_entity))
        .route("/v1/entities/:id/persistence", post(persistence::request))
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
        .route("/v1/providers/local", post(routes::set_local_provider))
        .route(
            "/v1/providers/routing",
            get(model_routing::get).post(model_routing::choose),
        )
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
        .layer(axum::middleware::from_fn_with_state(
            process_identity,
            profile_runtime::require_identity,
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
    axum::serve(listener, app)
        .with_graceful_shutdown(async move { shutdown.notified().await })
        .await?;

    Ok(())
}
