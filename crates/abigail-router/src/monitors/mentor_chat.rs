//! Compatibility wrapper for monitor module path.
//!
//! Canonical implementation currently lives under `crate::monitor::mentor_chat`.

use abigail_streaming::{StreamBroker, SubscriptionHandle};
use std::sync::Arc;

pub use crate::monitor::mentor_chat::{
    inject_preprompt, request_enriched_preprompt, MentorChatEnvelope, MentorChatMonitor,
};

/// Compatibility startup helper for external callers expecting the `monitors` path.
pub async fn start_mentor_chat_monitor(
    broker: Arc<dyn StreamBroker>,
) -> anyhow::Result<SubscriptionHandle> {
    MentorChatMonitor::new(broker).spawn().await
}
