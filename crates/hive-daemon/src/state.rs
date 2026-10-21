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
    pub genesis: Arc<crate::genesis_store::GenesisStore>,
    pub genesis_media: Arc<crate::genesis_media::GenesisMediaService>,
    /// Per-install credential for the desktop Hive app. See [`crate::access`].
    pub control_token: Arc<String>,
    pub genesis_control: Arc<crate::genesis::GenesisControl>,
    pub identity_manager: Arc<IdentityManager>,
    pub hive: Arc<Hive>,
    /// Hive-level secrets vault (shared across all agents).
    pub hive_secrets: Arc<Mutex<SecretsVault>>,
    /// Current externally reachable Hive URL for runtime leases.
    pub hive_url: String,
    /// In-memory runtime supervision and assignment control plane.
    pub runtime_control: Arc<Mutex<RuntimeControlPlane>>,
    /// Local URL of the running Hive helper entity-daemon, if up. Updated by the
    /// supervisor; surfaced in `/v1/status` and used by the Hive app's helper chat.
    pub helper_url: Arc<Mutex<Option<String>>>,
    /// Spawns/stops/reuses on-demand family entity-daemons.
    pub supervisor: Arc<HiveSupervisor>,
}
