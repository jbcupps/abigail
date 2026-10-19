//! Persistent DevOps Forge worker entrypoint.
//!
//! Runtime entrypoint for static proposal validation in `crate::DevopsForgeWorker`.
//! The separate trusted `crate::process_forge_request` writer is never invoked
//! by broker messages. Validation does not imply fitness or application.

use std::sync::Arc;

use abigail_streaming::{StreamBroker, SubscriptionHandle};

/// Runtime wrapper around the validation-only DevOps Forge worker.
pub struct PersistentForgeWorker {
    inner: crate::DevopsForgeWorker,
}

impl PersistentForgeWorker {
    pub fn new(broker: Arc<dyn StreamBroker>) -> Self {
        Self {
            inner: crate::DevopsForgeWorker::new(broker),
        }
    }

    pub async fn spawn(self) -> anyhow::Result<SubscriptionHandle> {
        self.inner.spawn().await
    }
}

/// Minimal worker entrypoint for runtime callers that want a single start API.
pub async fn start(broker: Arc<dyn StreamBroker>) -> anyhow::Result<SubscriptionHandle> {
    spawn_persistent_worker(broker).await
}

/// Spawn the persistent forge worker subscribed to `topic.skill.forge.request`.
pub async fn spawn_persistent_worker(
    broker: Arc<dyn StreamBroker>,
) -> anyhow::Result<SubscriptionHandle> {
    PersistentForgeWorker::new(broker).spawn().await
}
