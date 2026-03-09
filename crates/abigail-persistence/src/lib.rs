pub mod client;
pub mod encryption;
pub mod migration;
pub mod schema;

use serde::{Deserialize, Serialize};

pub use client::{EntityScope, PersistenceError, PersistenceHandle, QueryBinding};
pub use encryption::ScopedCipher;
pub use migration::{migrate_legacy_layout, MigrationReport, RecoverySnapshot};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReflectionScore {
    pub deontic: f32,
    pub areteological: f32,
    pub teleological: f32,
    pub summary: Option<String>,
}
