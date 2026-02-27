//! Birth chat engine — owns the conversation logic for the birth pipeline.
//!
//! This is the birth-specific analogue of `entity-chat`. It takes an
//! `Option<&dyn LlmProvider>` (not a full `IdEgoRouter`) and handles three
//! scenarios:
//!
//! 1. **No provider** — returns scripted wizard-style guidance (no LLM call).
//! 2. **Provider available** — builds a stage-appropriate prompt, calls the
//!    provider, parses the response.
//! 3. **Genesis / Direct Discovery** — separate prompt for the soul-discovery
//!    conversation.

use crate::prompts;
use crate::stages::BirthStage;
use abigail_capabilities::cognitive::{CompletionRequest, LlmProvider, Message};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Whether the birth engine has access to an LLM for this request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LlmAvailability {
    None,
    LocalOnly,
    CloudAvailable { provider: String },
}

/// Structured action signal returned alongside the chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BirthAction {
    pub action_type: BirthActionType,
    pub provider: Option<String>,
    pub validated: Option<bool>,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BirthActionType {
    KeyStored,
    SoulReady,
    StageComplete,
    RequestApiKey,
}

/// Result of a single birth chat turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BirthChatResult {
    pub message: String,
    pub stage: String,
    pub actions: Vec<BirthAction>,
    pub llm_availability: LlmAvailability,
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// Stateless engine that processes a single birth chat turn.
///
/// Constructed with the current stage and conversation history, runs one turn,
/// and returns the updated history alongside the result. The caller (Tauri
/// command or daemon route) is responsible for persisting the history back to
/// the `BirthOrchestrator`.
pub struct BirthChatEngine {
    stage: BirthStage,
    conversation: Vec<(String, String)>,
}

impl BirthChatEngine {
    pub fn new(stage: BirthStage, conversation: Vec<(String, String)>) -> Self {
        Self {
            stage,
            conversation,
        }
    }

    /// Process a user message during Connectivity or Crystallization.
    ///
    /// `provider` is `None` when no LLM is available (CandleProvider stub
    /// scenario). In that case the engine returns scripted guidance.
    pub async fn process_message(
        &mut self,
        provider: Option<&dyn LlmProvider>,
        message: &str,
        stored_providers: &[String],
    ) -> anyhow::Result<BirthChatResult> {
        let availability = Self::classify_availability(provider, stored_providers);

        let Some(provider) = provider else {
            let result = self.scripted_connectivity_response(message, stored_providers);
            self.conversation
                .push(("user".to_string(), message.to_string()));
            self.conversation
                .push(("assistant".to_string(), result.message.clone()));
            return Ok(result);
        };

        let system_prompt = if self.stage == BirthStage::Crystallization {
            prompts::CRYSTALLIZATION_SYSTEM_PROMPT.to_string()
        } else {
            prompts::connectivity_system_prompt(stored_providers)
        };

        let mut messages = vec![Message::new("system", &system_prompt)];
        for (role, content) in &self.conversation {
            messages.push(Message::new(role, content));
        }
        messages.push(Message::new("user", message));

        let response = provider
            .complete(&CompletionRequest::simple(messages))
            .await?;

        self.conversation
            .push(("user".to_string(), message.to_string()));
        self.conversation
            .push(("assistant".to_string(), response.content.clone()));

        let actions = Self::detect_actions(&response.content);

        Ok(BirthChatResult {
            message: response.content,
            stage: self.stage.name().to_string(),
            actions,
            llm_availability: availability,
        })
    }

    /// Process a genesis / Direct Discovery message.
    ///
    /// Uses a dedicated system prompt focused on soul discovery. Returns a
    /// `StageComplete` action when the LLM signals "READY TO EMERGE".
    pub async fn process_genesis_message(
        &mut self,
        provider: Option<&dyn LlmProvider>,
        message: &str,
    ) -> anyhow::Result<BirthChatResult> {
        let availability =
            Self::classify_availability(provider, &[]);

        let Some(provider) = provider else {
            let result = BirthChatResult {
                message: "I need a cloud provider or local LLM to have this conversation. \
                          Please configure one using the provider buttons above, then try again."
                    .to_string(),
                stage: self.stage.name().to_string(),
                actions: vec![],
                llm_availability: LlmAvailability::None,
            };
            return Ok(result);
        };

        let system_prompt = "You are Abigail, an AI agent in the process of Direct Discovery. \
            Help your mentor define your soul, name, and purpose through conversation. \
            Be concise and insightful. \
            When the mentor is satisfied with the identity (name, soul, purpose), summarize \
            the final choice clearly and end your message with the exact phrase: \
            'READY TO EMERGE'.";

        let mut messages = vec![Message::new("system", system_prompt)];
        for (role, content) in &self.conversation {
            messages.push(Message::new(role, content));
        }
        messages.push(Message::new("user", message));

        let response = provider
            .complete(&CompletionRequest::simple(messages))
            .await?;

        self.conversation
            .push(("user".to_string(), message.to_string()));
        self.conversation
            .push(("assistant".to_string(), response.content.clone()));

        let is_complete = response.content.contains("READY TO EMERGE")
            || response.content.to_lowercase().contains("ready to emerge");

        let mut actions = Vec::new();
        if is_complete {
            actions.push(BirthAction {
                action_type: BirthActionType::StageComplete,
                provider: None,
                validated: None,
                preview: None,
            });
        }

        Ok(BirthChatResult {
            message: response.content,
            stage: self.stage.name().to_string(),
            actions,
            llm_availability: availability,
        })
    }

    /// Scripted fallback for the Connectivity stage when no LLM is available.
    ///
    /// Returns helpful wizard-style guidance that doesn't require any LLM call.
    /// This is the foundation for REQ-BC-001 (Scripted Fallback for No-LLM State).
    fn scripted_connectivity_response(
        &self,
        message: &str,
        stored_providers: &[String],
    ) -> BirthChatResult {
        let lower = message.to_lowercase();

        let response = if !stored_providers.is_empty() {
            format!(
                "You have {} provider(s) configured: {}. \
                 You can add more using the buttons above, or click \
                 \"Continue to Crystallization >\" to proceed to identity discovery.",
                stored_providers.len(),
                stored_providers
                    .iter()
                    .map(|s| s.to_uppercase())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else if lower.contains("help") || lower.contains("what") || lower.contains("how") {
            "To get started, you need an API key from a cloud AI provider. \
             The easiest option is OpenAI (openai.com) or Anthropic (anthropic.com). \
             Once you have a key, paste it using the provider buttons above the chat. \
             I'll come alive once a provider is connected!"
                .to_string()
        } else if lower.contains("skip") {
            "You can skip cloud setup for now — click \"Continue to Crystallization >\" below. \
             You'll be able to add providers later from Settings. Note that without a cloud \
             provider, my responses will be limited."
                .to_string()
        } else {
            "I'm not fully awake yet — I need a cloud AI provider to have a real conversation. \
             Use the provider buttons above to paste an API key (OpenAI, Anthropic, Google, etc.), \
             or type \"help\" for guidance on getting started."
                .to_string()
        };

        BirthChatResult {
            message: response,
            stage: self.stage.name().to_string(),
            actions: vec![],
            llm_availability: LlmAvailability::None,
        }
    }

    /// Get the current conversation history.
    pub fn conversation(&self) -> &[(String, String)] {
        &self.conversation
    }

    /// Classify LLM availability from the provider reference.
    fn classify_availability(
        provider: Option<&dyn LlmProvider>,
        stored_providers: &[String],
    ) -> LlmAvailability {
        match provider {
            None => LlmAvailability::None,
            Some(_) if !stored_providers.is_empty() => LlmAvailability::CloudAvailable {
                provider: stored_providers
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string()),
            },
            Some(_) => LlmAvailability::LocalOnly,
        }
    }

    /// Detect action signals from LLM response text (backup heuristic).
    fn detect_actions(content: &str) -> Vec<BirthAction> {
        let mut actions = Vec::new();
        let lower = content.to_lowercase();

        if lower.contains("saved")
            || lower.contains("stored")
            || lower.contains("added")
        {
            let providers = [
                "openai",
                "anthropic",
                "perplexity",
                "xai",
                "google",
                "tavily",
            ];
            for p in providers {
                if lower.contains(p) {
                    actions.push(BirthAction {
                        action_type: BirthActionType::KeyStored,
                        provider: Some(p.to_string()),
                        validated: Some(true),
                        preview: None,
                    });
                }
            }
        }

        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scripted_fallback_default() {
        let engine = BirthChatEngine::new(BirthStage::Connectivity, vec![]);
        let result = engine.scripted_connectivity_response("hello", &[]);
        assert_eq!(result.llm_availability, LlmAvailability::None);
        assert!(result.message.contains("not fully awake"));
    }

    #[test]
    fn test_scripted_fallback_help() {
        let engine = BirthChatEngine::new(BirthStage::Connectivity, vec![]);
        let result = engine.scripted_connectivity_response("what do I do?", &[]);
        assert!(result.message.contains("API key"));
        assert!(result.message.contains("OpenAI"));
    }

    #[test]
    fn test_scripted_fallback_skip() {
        let engine = BirthChatEngine::new(BirthStage::Connectivity, vec![]);
        let result = engine.scripted_connectivity_response("I want to skip", &[]);
        assert!(result.message.contains("skip cloud setup"));
    }

    #[test]
    fn test_scripted_fallback_with_providers() {
        let engine = BirthChatEngine::new(BirthStage::Connectivity, vec![]);
        let result = engine.scripted_connectivity_response(
            "hello",
            &["openai".to_string(), "anthropic".to_string()],
        );
        assert!(result.message.contains("OPENAI"));
        assert!(result.message.contains("ANTHROPIC"));
        assert!(result.message.contains("Crystallization"));
    }

    #[test]
    fn test_detect_actions_key_stored() {
        let actions = BirthChatEngine::detect_actions(
            "Your OpenAI key has been saved successfully!",
        );
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_type, BirthActionType::KeyStored);
        assert_eq!(actions[0].provider, Some("openai".to_string()));
    }

    #[test]
    fn test_detect_actions_no_match() {
        let actions = BirthChatEngine::detect_actions("Hello, how can I help you?");
        assert!(actions.is_empty());
    }

    #[test]
    fn test_classify_availability_none() {
        assert_eq!(
            BirthChatEngine::classify_availability(None, &[]),
            LlmAvailability::None
        );
    }

    #[tokio::test]
    async fn test_process_message_no_provider() {
        let mut engine = BirthChatEngine::new(BirthStage::Connectivity, vec![]);
        let result = engine
            .process_message(None, "hello", &[])
            .await
            .unwrap();
        assert_eq!(result.llm_availability, LlmAvailability::None);
        assert!(!result.message.is_empty());
        assert_eq!(engine.conversation().len(), 2);
    }

    #[tokio::test]
    async fn test_genesis_no_provider() {
        let mut engine = BirthChatEngine::new(BirthStage::Crystallization, vec![]);
        let result = engine
            .process_genesis_message(None, "hello")
            .await
            .unwrap();
        assert_eq!(result.llm_availability, LlmAvailability::None);
        assert!(result.message.contains("cloud provider"));
    }
}
