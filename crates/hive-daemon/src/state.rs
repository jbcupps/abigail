//! Hive daemon shared state.

use crate::local_auth::LocalAuth;
use crate::persistence::PersistenceRegistry;
use crate::runtime_registry::RuntimeControlPlane;
use crate::supervisor::HiveSupervisor;
use abigail_core::SecretsVault;
use abigail_hive::Hive;
use abigail_identity::IdentityManager;
use std::sync::{Arc, Mutex};

/// Shared state for all hive-daemon route handlers.
#[derive(Clone)]
pub struct HiveDaemonState {
    pub identity_manager: Arc<IdentityManager>,
    pub hive: Arc<Hive>,
    /// Hive-level secrets vault (shared across all agents).
    pub hive_secrets: Arc<Mutex<SecretsVault>>,
    /// Current externally reachable Hive URL for runtime leases.
    pub hive_url: String,
    /// In-memory runtime supervision and assignment control plane.
    pub runtime_control: Arc<Mutex<RuntimeControlPlane>>,
    /// Spawns/stops/reuses on-demand family entity-daemons.
    pub supervisor: Arc<HiveSupervisor>,
    /// Per-launch control-plane token (also enforced by middleware).
    pub local_auth: LocalAuth,
    /// The Hive-owned shared store. Child daemons reach it through
    /// `/v1/persistence/op` rather than opening the locked file themselves.
    pub persistence: Arc<PersistenceRegistry>,
    pub bootstrap: Arc<crate::bootstrap::Bootstrap>,
}
