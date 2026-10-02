pub mod client;
pub mod config;
pub mod encryption;
pub mod migration;
pub mod remote;
pub mod schema;

use serde::{Deserialize, Serialize};

pub use client::{EntityScope, PersistenceError, PersistenceHandle, QueryBinding};
pub use config::{ci_mode_enabled, TEST_MODE};
pub use encryption::ScopedCipher;
pub use migration::{migrate_legacy_layout, MigrationReport, RecoverySnapshot};
pub use remote::{configure_hive_transport, RemoteOperation, RemoteResponse};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReflectionScore {
    pub deontic: f32,
    pub areteological: f32,
    pub teleological: f32,
    pub summary: Option<String>,
}
