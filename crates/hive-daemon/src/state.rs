//! Hive daemon shared state.

use crate::runtime_registry::RuntimeControlPlane;
use crate::supervisor::HiveSupervisor;
use abigail_core::SecretsVault;
use abigail_hive::Hive;
use abigail_identity::IdentityManager;
use std::sync::{Arc, Mutex};

/// Shared state for all hive-daemon route handlers.
#[derive(Clone)]
pub struct HiveDaemonState {
    pub process_identity: Option<abigail_core::profile::ProcessIdentity>,
    pub shutdown: Arc<tokio::sync::Notify>,
    pub genesis: Arc<crate::genesis_store::GenesisStore>,
    pub genesis_media: Arc<crate::genesis_media::GenesisMediaService>,
    pub genesis_token: Arc<String>,
    pub genesis_control: Arc<crate::genesis::GenesisControl>,
    pub identity_manager: Arc<IdentityManager>,
    pub hive: Arc<Hive>,
    /// Hive-level secrets vault (shared across all agents).
    pub hive_secrets: Arc<Mutex<SecretsVault>>,
    /// Current externally reachable Hive URL for runtime leases.
    pub hive_url: String,
    /// In-memory runtime supervision and assignment control plane.
    pub runtime_control: Arc<Mutex<RuntimeControlPlane>>,
    /// The helper's URL and process identity change together under one lock.
    /// Updated by the supervisor and exposed in `/v1/status`.
    pub helper: Arc<Mutex<Option<hive_core::HelperInfo>>>,
    /// Spawns/stops/reuses on-demand family entity-daemons.
    pub supervisor: Arc<HiveSupervisor>,
}
