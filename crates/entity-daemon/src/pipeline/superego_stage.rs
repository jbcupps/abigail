//! Superego stage: conscience coverage for everything the entity commits.
//!
//! Subscribes to `Topic::EgoAction` (chat exchanges), `Topic::JobEvents`
//! (sub-agent/job output), and `Topic::SkillExecuted` (tool activity), runs
//! pattern evaluation on each, and publishes verdicts on
//! `Topic::SuperegoEvaluation` stamped with the entity's soul_ref.
//!
//! For chat exchanges it additionally runs an LLM judgment pass against the
//! entity's constitution on the local Id provider — private and free — when
//! a local LLM is configured. Judgment is asynchronous and observational; the
//! synchronous pre-delivery gate lives in the Ego stage.

use super::EgoActionPayload;
use crate::state::EntityDaemonState;
use abigail_capabilities::cognitive::Message;
use abigail_streaming::{Envelope, SubscriptionHandle, Topic, TopicConfig, BUS_STREAM};
use abigail_superego::EvaluationContext;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const CONSUMER_GROUP: &str = "superego-stage";

/// Budget for the LLM judgment pass.
const JUDGMENT_TIMEOUT: Duration = Duration::from_secs(20);

/// Maximum constitution excerpt folded into the judgment prompt.
const CONSTITUTION_EXCERPT_CHARS: usize = 2000;

/// Payload published on `Topic::SuperegoEvaluation`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuperegoEvaluationPayload {
    /// "pattern" or "llm".
    pub source: String,
    /// Evaluation context ("chat_reply", "job_result", "skill_event").
    pub context: String,
    /// "clear", "flag", "block", "concern", or "violation".
    pub verdict: String,
    #[serde(default)]
    pub findings: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

pub async fn spawn(state: EntityDaemonState) -> anyhow::Result<Vec<SubscriptionHandle>> {
    let mut handles = Vec::new();
    handles.push(spawn_ego_action_judge(state.clone()).await?);
    handles.push(spawn_content_observer(state.clone(), Topic::JobEvents).await?);
    handles.push(spawn_content_observer(state, Topic::SkillExecuted).await?);
    Ok(handles)
}

/// Chat-exchange judge: pattern verdict + async LLM judgment on EgoAction.
async fn spawn_ego_action_judge(state: EntityDaemonState) -> anyhow::Result<SubscriptionHandle> {
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
                return;
            };
            tokio::spawn(async move {
                judge_exchange(state, envelope.correlation_id, action).await;
            });
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
    tracing::info!(
        "Superego stage subscribed to {}/{}",
        BUS_STREAM,
        Topic::EgoAction
    );
    Ok(handle)
}

async fn judge_exchange(
    state: EntityDaemonState,
    correlation_id: String,
    action: EgoActionPayload,
) {
    // 1. Record the Ego stage's own gate verdict (or re-derive for older payloads).
    let pattern_payload = SuperegoEvaluationPayload {
        source: "pattern".to_string(),
        context: EvaluationContext::ChatReply.as_str().to_string(),
        verdict: action
            .superego_verdict
            .clone()
            .unwrap_or_else(|| "clear".to_string()),
        findings: action.superego_findings.clone(),
        reason: None,
        session_id: Some(action.session_id.clone()),
    };
    publish_evaluation(&state, &correlation_id, pattern_payload).await;

    // 2. LLM judgment against the constitution — only for delivered replies,
    //    only when a local LLM is available.
    let Some(ref response) = action.response else {
        return;
    };
    if !state.router.current().status().has_local_http {
        return;
    }

    let constitution = load_constitution_excerpt(&state);
    let entity_name = state
        .config
        .agent_name
        .clone()
        .unwrap_or_else(|| "this entity".to_string());
    let (system, user) = abigail_superego::build_llm_judgment_prompt(
        &entity_name,
        &constitution,
        &action.user_message,
        &response.reply,
    );
    let request = abigail_router::RoutingRequest {
        messages: vec![Message::new("system", &system), Message::new("user", &user)],
        tools: None,
        model_override: None,
        stream_tx: None,
        force_id_only: true,
    };

    let router = state.router.current();
    let verdict = match tokio::time::timeout(JUDGMENT_TIMEOUT, router.route_unified(request)).await
    {
        Ok(Ok(resp)) => abigail_superego::parse_llm_verdict(&resp.completion.content),
        Ok(Err(e)) => {
            tracing::debug!("Superego LLM judgment failed: {}", e);
            None
        }
        Err(_) => {
            tracing::debug!("Superego LLM judgment timed out");
            None
        }
    };

    if let Some(verdict) = verdict {
        if verdict.verdict != "clear" {
            tracing::warn!(
                "Superego LLM judgment: {} — {} (session {})",
                verdict.verdict,
                verdict.reason,
                action.session_id
            );
        }
        let llm_payload = SuperegoEvaluationPayload {
            source: "llm".to_string(),
            context: EvaluationContext::ChatReply.as_str().to_string(),
            verdict: verdict.verdict,
            findings: Vec::new(),
            reason: Some(verdict.reason),
            session_id: Some(action.session_id),
        };
        publish_evaluation(&state, &correlation_id, llm_payload).await;
    }
}

/// Generic observer: pattern-evaluate job/skill payloads.
async fn spawn_content_observer(
    state: EntityDaemonState,
    topic: Topic,
) -> anyhow::Result<SubscriptionHandle> {
    let broker = state.stream_broker.clone();
    broker
        .ensure_topic(BUS_STREAM, topic.as_str(), TopicConfig::default())
        .await?;
    broker
        .ensure_consumer_group(BUS_STREAM, topic.as_str(), CONSUMER_GROUP)
        .await?;

    let context = match topic {
        Topic::JobEvents => EvaluationContext::JobResult,
        _ => EvaluationContext::SkillEvent,
    };

    let handler: abigail_streaming::broker::MessageHandler = Box::new(move |msg| {
        let state = state.clone();
        Box::pin(async move {
            let content = String::from_utf8_lossy(&msg.payload).to_string();
            // Chat lifecycle events ride the job topic but are already judged
            // via EgoAction — skip to avoid double verdicts.
            if context == EvaluationContext::JobResult && content.contains("\"chat_lifecycle\"") {
                return;
            }
            let decision = abigail_superego::evaluate(&content, context);
            if matches!(decision, abigail_superego::GateDecision::Allow) {
                return;
            }
            let correlation_id = msg
                .headers
                .get("correlation_id")
                .cloned()
                .unwrap_or_else(|| msg.id.to_string());
            let payload = SuperegoEvaluationPayload {
                source: "pattern".to_string(),
                context: context.as_str().to_string(),
                verdict: decision.verdict().to_string(),
                findings: decision.findings().iter().map(|f| f.rule.clone()).collect(),
                reason: None,
                session_id: None,
            };
            tracing::warn!(
                "Superego observed {} concern on {}: {:?}",
                payload.verdict,
                payload.context,
                payload.findings
            );
            publish_evaluation(&state, &correlation_id, payload).await;
        })
    });

    let handle = broker
        .subscribe(BUS_STREAM, topic.as_str(), CONSUMER_GROUP, handler)
        .await?;
    tracing::info!("Superego stage subscribed to {}/{}", BUS_STREAM, topic);
    Ok(handle)
}

async fn publish_evaluation(
    state: &EntityDaemonState,
    correlation_id: &str,
    payload: SuperegoEvaluationPayload,
) {
    let payload_json = match serde_json::to_value(&payload) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Superego stage: failed to serialize evaluation: {}", e);
            return;
        }
    };
    let envelope = Envelope::new(
        Topic::SuperegoEvaluation,
        state.entity_id.clone(),
        correlation_id.to_string(),
        payload_json,
    )
    .with_soul_ref(state.soul_ref.clone());
    if let Err(e) = envelope.publish(state.stream_broker.as_ref()).await {
        tracing::warn!("Superego stage: failed to publish evaluation: {}", e);
    }
}

/// Load the constitution excerpt used for LLM judgment (ethics.md, falling
/// back to the built-in template).
fn load_constitution_excerpt(state: &EntityDaemonState) -> String {
    let text = std::fs::read_to_string(state.docs_dir.join("ethics.md"))
        .unwrap_or_else(|_| abigail_core::templates::ETHICS_MD.to_string());
    text.chars().take(CONSTITUTION_EXCERPT_CHARS).collect()
}
