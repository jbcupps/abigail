//! Ego stage: turns an Id reaction into a committed response.
//!
//! Subscribes to `Topic::IdReaction`, claims the turn's runtime context from
//! the [`TurnRegistry`], builds the contextual message list from memory, runs
//! the provider call (tool loop or streaming pipeline), then publishes
//! `Topic::EgoAction` plus a `Topic::EgoDeliberation` trace and delivers the
//! outcome to the waiting HTTP handler.

use super::{EgoActionPayload, IdReactionPayload, TurnContext};
use crate::state::EntityDaemonState;
use abigail_streaming::{Envelope, SubscriptionHandle, Topic, TopicConfig, BUS_STREAM};
use entity_core::ChatResponse;

const CONSUMER_GROUP: &str = "ego-stage";

pub async fn spawn(state: EntityDaemonState) -> anyhow::Result<SubscriptionHandle> {
    let broker = state.stream_broker.clone();
    broker
        .ensure_topic(
            BUS_STREAM,
            Topic::IdReaction.as_str(),
            TopicConfig::default(),
        )
        .await?;
    broker
        .ensure_consumer_group(BUS_STREAM, Topic::IdReaction.as_str(), CONSUMER_GROUP)
        .await?;

    let handler: abigail_streaming::broker::MessageHandler = Box::new(move |msg| {
        let state = state.clone();
        Box::pin(async move {
            let Ok(envelope) = Envelope::from_message(&msg) else {
                return;
            };
            let Ok(payload) = serde_json::from_value::<IdReactionPayload>(envelope.payload.clone())
            else {
                tracing::warn!("Ego stage: malformed IdReaction payload");
                return;
            };
            let Some(ctx) = state.turns.take(&envelope.correlation_id) else {
                // Handler timed out or this reaction belongs to no live turn.
                tracing::warn!(
                    "Ego stage: no live turn for correlation {}",
                    envelope.correlation_id
                );
                return;
            };
            // Run each turn on its own task so a slow provider never
            // serializes unrelated turns behind it.
            tokio::spawn(async move {
                respond(state, envelope.correlation_id, payload, ctx).await;
            });
        })
    });

    let handle = broker
        .subscribe(
            BUS_STREAM,
            Topic::IdReaction.as_str(),
            CONSUMER_GROUP,
            handler,
        )
        .await?;
    tracing::info!(
        "Ego stage subscribed to {}/{}",
        BUS_STREAM,
        Topic::IdReaction
    );
    Ok(handle)
}

async fn respond(
    state: EntityDaemonState,
    correlation_id: String,
    payload: IdReactionPayload,
    ctx: TurnContext,
) {
    let TurnContext {
        session_messages,
        stream_tx,
        cancel,
        done,
    } = ctx;

    let budget = entity_chat::ContextBudget::default();
    let messages = entity_chat::build_contextual_messages_with_memory(
        &state.memory,
        &payload.session_id,
        &payload.system_prompt,
        session_messages,
        &payload.message,
        &budget,
    );
    let tools = entity_chat::build_tool_definitions(&state.registry);
    // Snapshot the active router for this turn (hot-swap applies on the next).
    let router = state.router.current();
    // Chat turns are depth 0; jobs submitted from this turn nest below it and
    // inherit the turn's correlation id for trace continuity.
    let job_ctx = entity_chat::JobContext {
        depth: 0,
        correlation_id: Some(correlation_id.clone()),
    };

    let result: anyhow::Result<entity_chat::ToolUseResult> = if let Some(stream_tx) = stream_tx {
        // Tokens pass through the incremental superego gate on their way to
        // the client: the relay cuts the stream on the first blocking finding
        // so blocked content never reaches the SSE channel (see stream_gate).
        let gated_tx = super::stream_gate::spawn_gated_relay(stream_tx);
        let pipeline_fut = entity_chat::stream_chat_pipeline(
            &router,
            &state.executor,
            messages,
            tools,
            gated_tx,
            payload.model_override.clone(),
            Some(&job_ctx),
        );
        tokio::pin!(pipeline_fut);
        let cancelled = async {
            match cancel.as_ref() {
                Some(token) => token.cancelled().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            res = &mut pipeline_fut => res.map(|p| entity_chat::ToolUseResult {
                content: p.content,
                tool_calls_made: p.tool_calls_made,
                execution_trace: p.execution_trace,
            }),
            _ = cancelled => Err(anyhow::anyhow!("Interrupted by user")),
        }
    } else if tools.is_empty() {
        router
            .route_unified(abigail_router::RoutingRequest {
                messages,
                tools: None,
                model_override: payload.model_override.clone(),
                stream_tx: None,
                force_id_only: false,
            })
            .await
            .map(|r| entity_chat::ToolUseResult {
                content: r.completion.content,
                tool_calls_made: Vec::new(),
                execution_trace: r.trace,
            })
    } else {
        entity_chat::run_tool_use_loop_with_model_override(
            &router,
            &state.executor,
            messages,
            tools,
            payload.model_override.clone(),
            Some(&job_ctx),
        )
        .await
    };

    let action = match result {
        Ok(tool_result) => {
            let tier = tool_result.tier().map(|s| s.to_string());
            let model_used = tool_result.model_used().map(|s| s.to_string());
            let complexity_score = tool_result.complexity_score();
            let provider = tool_result
                .execution_trace
                .as_ref()
                .and_then(|t| t.final_provider())
                .map(|s| s.to_string())
                .or_else(|| Some(entity_chat::provider_label(&router)));

            // Publish the deliberation trace for audit before committing.
            if let Some(ref trace) = tool_result.execution_trace {
                if let Ok(trace_json) = serde_json::to_value(trace) {
                    let deliberation = Envelope::new(
                        Topic::EgoDeliberation,
                        state.entity_id.clone(),
                        correlation_id.clone(),
                        trace_json,
                    )
                    .with_soul_ref(state.soul_ref.clone());
                    if let Err(e) = deliberation.publish(state.stream_broker.as_ref()).await {
                        tracing::debug!("Ego stage: failed to publish deliberation: {}", e);
                    }
                }
            }

            // Pre-delivery superego gate: secrets and SSNs never reach the
            // mentor; lesser concerns are flagged for the audit trail. On the
            // streaming path the stream_gate relay has already cut the live
            // token flow at the first blocking finding; this full-content
            // pass renders the authoritative verdict for the committed
            // action, the journal, and the handler's final event.
            let gate = abigail_superego::evaluate(
                &tool_result.content,
                abigail_superego::EvaluationContext::ChatReply,
            );
            let findings: Vec<String> = gate.findings().iter().map(|f| f.rule.clone()).collect();

            match gate {
                abigail_superego::GateDecision::Block(_) => EgoActionPayload {
                    session_id: payload.session_id.clone(),
                    status: "blocked".to_string(),
                    error: Some(format!(
                        "Response withheld by superego: {}",
                        findings.join(", ")
                    )),
                    response: None,
                    user_message: payload.message.clone(),
                    superego_verdict: Some("block".to_string()),
                    superego_findings: findings,
                },
                gate => {
                    let response = ChatResponse {
                        reply: tool_result.content,
                        provider,
                        tool_calls_made: tool_result.tool_calls_made,
                        tier,
                        model_used,
                        complexity_score,
                        execution_trace: tool_result.execution_trace,
                        session_id: Some(payload.session_id.clone()),
                    };
                    EgoActionPayload {
                        session_id: payload.session_id.clone(),
                        status: "success".to_string(),
                        error: None,
                        response: Some(response),
                        user_message: payload.message.clone(),
                        superego_verdict: Some(gate.verdict().to_string()),
                        superego_findings: findings,
                    }
                }
            }
        }
        Err(e) => EgoActionPayload {
            session_id: payload.session_id.clone(),
            status: "error".to_string(),
            error: Some(e.to_string()),
            response: None,
            user_message: payload.message.clone(),
            superego_verdict: None,
            superego_findings: Vec::new(),
        },
    };

    // Publish the committed action for the journal, superego, and any other
    // observers — regardless of whether the handler is still waiting.
    match serde_json::to_value(&action) {
        Ok(action_json) => {
            let envelope = Envelope::new(
                Topic::EgoAction,
                state.entity_id.clone(),
                correlation_id.clone(),
                action_json,
            )
            .with_soul_ref(state.soul_ref.clone());
            if let Err(e) = envelope.publish(state.stream_broker.as_ref()).await {
                tracing::error!("Ego stage: failed to publish EgoAction: {}", e);
            }
        }
        Err(e) => tracing::error!("Ego stage: failed to serialize EgoAction: {}", e),
    }

    // Deliver the outcome to the waiting handler.
    let outcome = match (action.status.as_str(), action.response) {
        ("success", Some(response)) => Ok(response),
        _ => Err(action
            .error
            .unwrap_or_else(|| "ego stage failed without detail".to_string())),
    };
    if done.send(outcome).is_err() {
        tracing::debug!(
            "Ego stage: handler for correlation {} no longer waiting",
            correlation_id
        );
    }
}
