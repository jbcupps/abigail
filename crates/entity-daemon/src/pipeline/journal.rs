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
            if let Err(error) = record(state, action).await {
                tracing::error!("Journal could not queue Runtime outbox events: {error}");
            }
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

async fn record(state: EntityDaemonState, action: EgoActionPayload) -> Result<(), String> {
    let mut outbox_errors = 0;
    match (action.status.as_str(), action.response) {
        ("success", Some(response)) => {
            // The Ego stage persisted this reply before acknowledging the turn.

            if let Err(error) = state.queue_outbox_record(
                "chat_assistant_turn",
                serde_json::json!({
                    "session_id": action.session_id.clone(),
                    "reply": response.reply.clone(),
                    "provider": response.provider.clone(),
                    "model_used": response.model_used.clone(),
                }),
            ) {
                outbox_errors += 1;
                tracing::error!(
                    session_id = %action.session_id,
                    "Committed assistant outbox event could not be queued: {error}"
                );
            }
            for tool in response
                .tool_calls_made
                .iter()
                .filter(|record| record.success)
            {
                if let Err(error) = state.queue_outbox_record(
                    "skill_used",
                    serde_json::json!({
                        "session_id": action.session_id.clone(),
                        "skill_id": tool.skill_id.clone(),
                        "tool_name": tool.tool_name.clone(),
                    }),
                ) {
                    outbox_errors += 1;
                    tracing::error!(
                        session_id = %action.session_id,
                        skill_id = %tool.skill_id,
                        tool_name = %tool.tool_name,
                        "Committed skill-use journey event could not be queued: {error}"
                    );
                }
            }

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
    if outbox_errors == 0 {
        Ok(())
    } else {
        Err(format!(
            "Runtime outbox rejected {outbox_errors} committed event(s)"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn journal_does_not_requeue_events_staged_before_a_successful_action() {
        let state = crate::routes::tests::build_state();
        for (skill_id, tool_name) in [
            ("com.abigail.calendar", "list_events"),
            ("com.abigail.weather", "forecast"),
        ] {
            state
                .queue_outbox_record(
                    "skill_used",
                    serde_json::json!({
                        "session_id": "session-1",
                        "skill_id": skill_id,
                        "tool_name": tool_name,
                    }),
                )
                .unwrap();
        }
        let staged = state.outbox.snapshot().unwrap();
        let response = entity_core::ChatResponse {
            reply: "private assistant reply".to_string(),
            provider: Some("fixture".to_string()),
            tool_calls_made: vec![
                entity_core::ToolCallRecord {
                    skill_id: "com.abigail.calendar".to_string(),
                    tool_name: "list_events".to_string(),
                    success: true,
                },
                entity_core::ToolCallRecord {
                    skill_id: "com.abigail.mail".to_string(),
                    tool_name: "send_message".to_string(),
                    success: false,
                },
                entity_core::ToolCallRecord {
                    skill_id: "com.abigail.weather".to_string(),
                    tool_name: "forecast".to_string(),
                    success: true,
                },
            ],
            tier: None,
            model_used: None,
            complexity_score: None,
            execution_trace: None,
            session_id: Some("session-1".to_string()),
        };
        record(
            state.clone(),
            EgoActionPayload {
                session_id: "session-1".to_string(),
                status: "success".to_string(),
                error: None,
                response: Some(response),
                user_message: "private mentor prompt".to_string(),
                superego_verdict: Some("clear".to_string()),
                superego_findings: vec![],
            },
        )
        .await
        .unwrap();

        let records = state.outbox.snapshot().unwrap();
        assert_eq!(
            serde_json::to_value(&records).unwrap(),
            serde_json::to_value(&staged).unwrap(),
            "the auto-committing journal must not duplicate pre-staged outbox events"
        );
        for event in records {
            let serialized = event.payload.to_string();
            assert!(!serialized.contains("private assistant reply"));
            assert!(!serialized.contains("private mentor prompt"));
            assert!(event.payload.get("reply").is_none());
            assert!(event.payload.get("message").is_none());
            assert!(event.payload.get("content").is_none());
        }
    }
}
