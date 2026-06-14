//! Governance: applies hive-originated configuration changes over the bus.
//!
//! Subscribes to `Topic::GovernanceInbound`. The runtime supervision loop
//! polls the Hive and publishes a refresh envelope when assignments change;
//! this subscriber applies them live — no entity-daemon restart required.
//! Dispatch is on the payload's `kind` field so future governance kinds
//! (policy refresh, provider refresh) slot in without architecture change.

use crate::state::EntityDaemonState;
use abigail_streaming::{Envelope, SubscriptionHandle, Topic, TopicConfig, BUS_STREAM};
use hive_core::SkillAssignment;

const CONSUMER_GROUP: &str = "governance";

/// Governance payload kind for live skill assignment refresh.
pub const KIND_SKILL_ASSIGNMENTS_REFRESH: &str = "skill_assignments.refresh";

/// Governance payload kind for live provider config refresh (router hot-swap).
pub const KIND_PROVIDER_REFRESH: &str = "provider.refresh";

pub async fn spawn(state: EntityDaemonState) -> anyhow::Result<SubscriptionHandle> {
    let broker = state.stream_broker.clone();
    broker
        .ensure_topic(
            BUS_STREAM,
            Topic::GovernanceInbound.as_str(),
            TopicConfig::default(),
        )
        .await?;
    broker
        .ensure_consumer_group(
            BUS_STREAM,
            Topic::GovernanceInbound.as_str(),
            CONSUMER_GROUP,
        )
        .await?;

    let handler: abigail_streaming::broker::MessageHandler = Box::new(move |msg| {
        let state = state.clone();
        Box::pin(async move {
            let Ok(envelope) = Envelope::from_message(&msg) else {
                return;
            };
            let kind = envelope
                .payload
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match kind {
                KIND_SKILL_ASSIGNMENTS_REFRESH => {
                    apply_skill_assignments(&state, &envelope.payload).await;
                }
                KIND_PROVIDER_REFRESH => {
                    apply_provider_refresh(&state, &envelope.payload).await;
                }
                other => {
                    tracing::debug!("Governance: unhandled inbound kind '{}'", other);
                }
            }
        })
    });

    let handle = broker
        .subscribe(
            BUS_STREAM,
            Topic::GovernanceInbound.as_str(),
            CONSUMER_GROUP,
            handler,
        )
        .await?;
    tracing::info!(
        "Governance subscribed to {}/{}",
        BUS_STREAM,
        Topic::GovernanceInbound
    );
    Ok(handle)
}

/// Rebuild the router from a hive-resolved provider config and hot-swap it.
/// All call sites fetch the router through [`crate::state::RouterHandle`], so
/// the new provider takes effect on the next turn — no restart.
async fn apply_provider_refresh(state: &EntityDaemonState, payload: &serde_json::Value) {
    let provider_config = match serde_json::from_value::<hive_core::ProviderConfig>(
        payload
            .get("provider_config")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    ) {
        Ok(config) => config,
        Err(e) => {
            tracing::warn!("Governance: malformed provider refresh: {}", e);
            return;
        }
    };

    let ego_label = provider_config.ego_provider_name.clone();
    let router = crate::router_build::build_router(provider_config).await;
    state.router.swap(std::sync::Arc::new(router));
    tracing::info!(
        "Governance: router hot-swapped (ego={:?}) — provider change applied live",
        ego_label
    );
}

async fn apply_skill_assignments(state: &EntityDaemonState, payload: &serde_json::Value) {
    let assignments = match serde_json::from_value::<Vec<SkillAssignment>>(
        payload
            .get("assignments")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    ) {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("Governance: malformed skill assignment refresh: {}", e);
            return;
        }
    };

    let count = assignments.len();
    {
        let mut current = state.skill_assignments.write().await;
        *current = assignments.clone();
    }
    for assignment in &assignments {
        state
            .push_skill_ack(entity_core::SkillApplyAcknowledgement {
                skill_id: assignment.skill_id.clone(),
                status: "assignment_refreshed".to_string(),
                applied_at_utc: chrono::Utc::now().to_rfc3339(),
            })
            .await;
    }
    tracing::info!(
        "Governance: applied {} skill assignment(s) from hive (live refresh)",
        count
    );
}
