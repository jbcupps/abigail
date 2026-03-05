//! Persistent DevOps Forge worker entrypoint.
//!
//! Canonical forge pipeline is implemented in `crate::DevopsForgeWorker` and
//! `crate::process_forge_request`. This module provides a dedicated worker path
//! so runtime callers can depend on `soul_forge::worker::*`.

use std::path::PathBuf;
use std::sync::Arc;

use abigail_streaming::{StreamBroker, SubscriptionHandle};

/// Compatibility wrapper around the canonical DevOps Forge worker.
pub struct PersistentForgeWorker {
    inner: crate::DevopsForgeWorker,
}

impl PersistentForgeWorker {
    pub fn new(broker: Arc<dyn StreamBroker>, skills_root: PathBuf) -> Self {
        Self {
            inner: crate::DevopsForgeWorker::new(broker, skills_root),
        }
    }

    pub async fn spawn(self) -> anyhow::Result<SubscriptionHandle> {
        self.inner.spawn().await
    }
}

/// Spawn the persistent forge worker subscribed to `topic.skill.forge.request`.
pub async fn spawn_persistent_worker(
    broker: Arc<dyn StreamBroker>,
    skills_root: PathBuf,
) -> anyhow::Result<SubscriptionHandle> {
    PersistentForgeWorker::new(broker, skills_root)
        .spawn()
        .await
}
