//! Hive daemon — control plane HTTP server for the Abigail Hive.
//!
//! Wraps `IdentityManager`, `Hive`, and `SecretsVault` behind an Axum REST API.
//! Listens on `--port` (default: the channel's first Hive port, 43141 for stable).

mod access;
mod app;
mod birth;
mod doctor;
mod genesis;
mod genesis_director;
mod genesis_http;
mod genesis_media;
mod genesis_retention;
mod genesis_store;
mod journey;
mod persistence;
mod routes;
mod runtime_registry;
mod state;
mod supervisor;

use abigail_core::{AppConfig, SecretsVault};
use abigail_hive::Hive;
use abigail_identity::IdentityManager;
use clap::Parser;
use state::HiveDaemonState;
use std::sync::{Arc, Mutex};

#[derive(Parser)]
#[command(name = "hive-daemon", about = "Abigail Hive control plane daemon")]
struct Cli {
    /// Port to listen on
    #[arg(long, default_value_t = *abigail_core::channel::current().hive_ports().start())]
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

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", cli.port)).await?;
    let local_addr = listener.local_addr()?;
    let hive_url = format!("http://{}", local_addr);

    // Capture the immortal Hive helper's id before `identity_manager` moves into
    // the daemon state.
    let hive_helper_id = identity_manager.hive_agent_id();
    let helper_url: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let supervisor = supervisor::HiveSupervisor::new(hive_url.clone(), data_root.clone());

    let state = HiveDaemonState {
        genesis: Arc::new(genesis_store::GenesisStore::new(_shared_store.clone())?),
        genesis_media: Arc::new(genesis_media::GenesisMediaService::new(
            &data_root,
            hive.clone(),
        )?),
        journeys: Arc::new(journey::JourneyStore::new(_shared_store.clone())?),
        control_token: Arc::new(access::load_or_create_control_token(&data_root)?),
        genesis_control: Arc::new(genesis::GenesisControl::default()),
        identity_manager,
        hive,
        hive_secrets,
        hive_url: hive_url.clone(),
        runtime_control: Arc::new(Mutex::new(runtime_registry::RuntimeControlPlane::default())),
        helper_url: helper_url.clone(),
        supervisor: supervisor.clone(),
    };

    let _genesis_worker = state.genesis_media.clone().start_worker();
    let _genesis_retention = genesis_retention::start_genesis_retention(state.clone());

    // Start the persistent Hive helper out-of-band: the immortal "Abigail Hive"
    // identity, run as its own entity-daemon. The control plane keeps serving
    // /health immediately while the helper warms up.
    match hive_helper_id {
        Ok(id) => {
            supervisor::supervise_hive_helper(id, hive_url.clone(), data_root.clone(), helper_url)
        }
        Err(e) => tracing::warn!("Hive helper not started: {}", e),
    }

    let app = app::router(state);

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
