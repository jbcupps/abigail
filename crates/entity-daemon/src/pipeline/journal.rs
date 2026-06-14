//! Journal: persists committed Ego actions and emits lifecycle events.
//!
//! Subscribes to `Topic::EgoAction` and handles the bookkeeping that used to
//! live inline in the chat handlers: archiving the assistant turn to memory,
//! queueing the outbox record for hive sync, triggering auto-archive, and
//! publishing chat lifecycle events.

use super::EgoActionPayload;
use crate::state::EntityDaemonState;
use abigail_streaming::{Envelope, SubscriptionHandle, Topic, TopicConfig, BUS_STREAM};

const CONSUMER_GROUP: &str = "journal";

pub async fn spawn(state: EntityDaemonState) -> anyhow::Result<SubscriptionHandle> {
    let broker = state.stream_broker.clone();
    broker
        .ensure_topic(
            BUS_STREAM,
            Topic::EgoAction.as_str(),
            TopicConfig::default(),
        )
        .await?;
    broker
        .ensure_consumer_group(BUS_STREAM, Topic::EgoAction.as_str(), CONSUMER_GROUP)
        .await?;

    let handler: abigail_streaming::broker::MessageHandler = Box::new(move |msg| {
        let state = state.clone();
        Box::pin(async move {
            let Ok(envelope) = Envelope::from_message(&msg) else {
                return;
            };
            let Ok(action) = serde_json::from_value::<EgoActionPayload>(envelope.payload.clone())
            else {
                tracing::warn!("Journal: malformed EgoAction payload");
                return;
            };
            record(state, action).await;
        })
    });

    let handle = broker
        .subscribe(
            BUS_STREAM,
            Topic::EgoAction.as_str(),
            CONSUMER_GROUP,
            handler,
        )
        .await?;
    tracing::info!("Journal subscribed to {}/{}", BUS_STREAM, Topic::EgoAction);
    Ok(handle)
}

async fn record(state: EntityDaemonState, action: EgoActionPayload) {
    match (action.status.as_str(), action.response) {
        ("success", Some(response)) => {
            // Archive the assistant turn (async, fire-and-forget via broker).
            let asst_turn = abigail_memory::ConversationTurn::new(
                &action.session_id,
                "assistant",
                &response.reply,
            )
            .with_metadata(
                response.provider.clone(),
                response.model_used.clone(),
                response.tier.clone(),
                response.complexity_score,
            );
            crate::memory_consumer::publish_turn(state.stream_broker.clone(), asst_turn);

            let _ = state.queue_outbox_record(
                "chat_assistant_turn",
                serde_json::json!({
                    "session_id": action.session_id.clone(),
                    "reply": response.reply.clone(),
                    "provider": response.provider.clone(),
                    "model_used": response.model_used.clone(),
                }),
            );

            state.maybe_auto_archive();

            crate::routes::publish_chat_lifecycle_event(
                state.stream_broker.clone(),
                action.session_id.clone(),
                state.entity_id.clone(),
                "chat_completed",
                serde_json::json!({
                    "provider": response.provider,
                    "model_used": response.model_used,
                    "tool_calls": response.tool_calls_made.len(),
                    "tool_summary": response.tool_calls_made.iter().map(|record| {
                        serde_json::json!({
                            "skill_id": record.skill_id,
                            "tool_name": record.tool_name,
                            "success": record.success,
                        })
                    }).collect::<Vec<_>>(),
                }),
            );
        }
        _ => {
            crate::routes::publish_chat_lifecycle_event(
                state.stream_broker.clone(),
                action.session_id.clone(),
                state.entity_id.clone(),
                "chat_failed",
                serde_json::json!({
                    "error": action.error.unwrap_or_else(|| "unknown".to_string()),
                }),
            );
        }
    }
}
