//! Hive daemon shared state.

use crate::runtime_registry::RuntimeControlPlane;
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
}
