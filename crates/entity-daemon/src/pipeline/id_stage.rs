//! Id stage: curates the system prompt for each mentor input.
//!
//! Subscribes to `Topic::MentorInput` (stage = "request"), synthesizes the
//! system prompt from the entity's soul documents, constitutional enrichment,
//! and an optional local-LLM personality consult, then publishes
//! `Topic::IdReaction` for the Ego stage.

use super::IdReactionPayload;
use crate::state::EntityDaemonState;
use abigail_capabilities::cognitive::Message;
use abigail_core::constitutional::{
    infer_id_context, infer_superego_context, load_minimal_preprompt,
};
use abigail_router::MentorChatEnvelope;
use abigail_streaming::{Envelope, SubscriptionHandle, Topic, TopicConfig, BUS_STREAM};
use std::time::Duration;

const CONSUMER_GROUP: &str = "id-stage";

/// Budget for the local Id personality consult; on timeout the turn proceeds
/// degraded (no personality signal) rather than stalling the pipeline.
const ID_CONSULT_TIMEOUT: Duration = Duration::from_secs(8);

/// Maximum length of the personality signal woven into the prompt.
const ID_SIGNAL_MAX_CHARS: usize = 400;

pub async fn spawn(state: EntityDaemonState) -> anyhow::Result<SubscriptionHandle> {
    let broker = state.stream_broker.clone();
    broker
        .ensure_topic(
            BUS_STREAM,
            Topic::MentorInput.as_str(),
            TopicConfig::default(),
        )
        .await?;
    broker
        .ensure_consumer_group(BUS_STREAM, Topic::MentorInput.as_str(), CONSUMER_GROUP)
        .await?;

    let handler: abigail_streaming::broker::MessageHandler = Box::new(move |msg| {
        let state = state.clone();
        Box::pin(async move {
            let Ok(envelope) = serde_json::from_slice::<MentorChatEnvelope>(&msg.payload) else {
                return;
            };
            // The mentor chat monitor republishes enriched envelopes on this
            // topic; the pipeline reacts to fresh requests only.
            if envelope.stage != "request" {
                return;
            }
            // Run each turn on its own task so a slow consult never serializes
            // unrelated turns behind it.
            tokio::spawn(async move {
                curate_and_publish(state, envelope).await;
            });
        })
    });

    let handle = broker
        .subscribe(
            BUS_STREAM,
            Topic::MentorInput.as_str(),
            CONSUMER_GROUP,
            handler,
        )
        .await?;
    tracing::info!(
        "Id stage subscribed to {}/{}",
        BUS_STREAM,
        Topic::MentorInput
    );
    Ok(handle)
}

async fn curate_and_publish(state: EntityDaemonState, envelope: MentorChatEnvelope) {
    let status = state.router.current().status();
    let lower = envelope.message.to_lowercase();
    let id_context = infer_id_context(&lower).to_string();
    let superego_context = infer_superego_context(&lower).to_string();

    // Constitutional enrichment (same source the mentor monitor uses).
    let enriched_preprompt = load_minimal_preprompt(&envelope.message).await.ok();

    // Base prompt: the persistent Hive helper gets an assistant persona focused
    // on helping the user run Abigail; family-facing entities use their soul
    // (CLI providers get the compressed form).
    let base_prompt = if state.config.is_hive {
        abigail_core::system_prompt::build_hive_helper_prompt(&state.config.agent_name)
    } else if status.mode == abigail_core::RoutingMode::CliOrchestrator {
        entity_chat::build_cli_system_prompt(
            &state.docs_dir,
            &state.config.agent_name,
            &state.registry,
            &state.instruction_registry,
            &envelope.message,
        )
    } else {
        let prompt = abigail_core::system_prompt::build_system_prompt(
            &state.docs_dir,
            &state.config.agent_name,
        );
        let runtime_ctx = entity_chat::RuntimeContext {
            provider_name: status.ego_provider.clone(),
            model_id: None,
            routing_mode: Some(format!("{:?}", status.mode)),
            tier: None,
            complexity_score: None,
            entity_name: state.config.agent_name.clone(),
            entity_id: Some(state.entity_id.clone()),
            has_local_llm: status.has_local_http,
            last_provider_change_at: None,
        };
        entity_chat::augment_system_prompt(
            &prompt,
            &state.registry,
            &state.instruction_registry,
            &envelope.message,
            &runtime_ctx,
            entity_chat::PromptMode::Full,
        )
    };
    let mut system_prompt = abigail_router::inject_preprompt(&base_prompt, enriched_preprompt);

    // Personality consult on the local Id provider (skipped when no local LLM).
    let (id_signal, id_consult) =
        consult_id(&state, status.has_local_http, &envelope.message).await;
    if let Some(ref signal) = id_signal {
        system_prompt.push_str("\n\n## Id Signal\n");
        system_prompt.push_str(signal);
        system_prompt.push('\n');
    }

    let payload = IdReactionPayload {
        session_id: envelope.session_id.clone(),
        message: envelope.message.clone(),
        model_override: envelope.selected_model.clone(),
        system_prompt,
        id_signal,
        id_consult: id_consult.to_string(),
        id_context,
        superego_context,
    };
    let payload_json = match serde_json::to_value(&payload) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Id stage: failed to serialize IdReactionPayload: {}", e);
            return;
        }
    };

    let reaction = Envelope::new(
        Topic::IdReaction,
        state.entity_id.clone(),
        envelope.correlation_id.clone(),
        payload_json,
    )
    .with_soul_ref(state.soul_ref.clone());

    if let Err(e) = reaction.publish(state.stream_broker.as_ref()).await {
        tracing::error!("Id stage: failed to publish IdReaction: {}", e);
    }
}

/// Ask the local Id provider for a one-sentence personality signal.
async fn consult_id(
    state: &EntityDaemonState,
    has_local_llm: bool,
    message: &str,
) -> (Option<String>, &'static str) {
    if !has_local_llm {
        return (None, "skipped");
    }

    let agent_name = state
        .config
        .agent_name
        .clone()
        .unwrap_or_else(|| "this entity".to_string());
    let consult_prompt = format!(
        "You are the Id — the instinctive personality core of {}. \
         Read the mentor's message and reply with ONE short sentence naming the \
         emotional tone and drive the entity should bring to its reply. \
         No preamble, no advice, no analysis.",
        agent_name
    );
    let request = abigail_router::RoutingRequest {
        messages: vec![
            Message::new("system", &consult_prompt),
            Message::new("user", message),
        ],
        tools: None,
        model_override: None,
        stream_tx: None,
        force_id_only: true,
    };

    let router = state.router.current();
    match tokio::time::timeout(ID_CONSULT_TIMEOUT, router.route_unified(request)).await {
        Ok(Ok(resp)) => {
            let text = resp.completion.content.trim().to_string();
            if text.is_empty() {
                (None, "empty")
            } else {
                let signal: String = text.chars().take(ID_SIGNAL_MAX_CHARS).collect();
                (Some(signal), "success")
            }
        }
        Ok(Err(e)) => {
            tracing::warn!("Id consult failed (continuing degraded): {}", e);
            (None, "error")
        }
        Err(_) => {
            tracing::warn!(
                "Id consult timed out after {:?} (continuing degraded)",
                ID_CONSULT_TIMEOUT
            );
            (None, "timeout")
        }
    }
}
