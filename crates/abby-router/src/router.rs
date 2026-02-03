//! Id/Ego router: classifies with Id (local), routes COMPLEX to Ego (cloud) when configured.

use abby_llm::{CandleProvider, CompletionRequest, CompletionResponse, LlmProvider, Message, OpenAiProvider};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteDecision {
    Routine,
    Complex,
}

/// Routes user messages: Id (local) classifies; ROUTINE stays local, COMPLEX goes to Ego if configured.
pub struct IdEgoRouter {
    id: Arc<CandleProvider>,
    ego: Option<Arc<OpenAiProvider>>,
}

impl IdEgoRouter {
    pub fn new(openai_api_key: Option<String>) -> Self {
        let ego = openai_api_key
            .filter(|k| !k.is_empty())
            .map(|k| Arc::new(OpenAiProvider::new(Some(k))));
        Self {
            id: Arc::new(CandleProvider::new()),
            ego,
        }
    }

    /// Classify with Id: ROUTINE or COMPLEX.
    pub async fn classify(&self, user_message: &str) -> anyhow::Result<RouteDecision> {
        let prompt = format!(
            "Classify this user request. Reply with exactly one word: ROUTINE or COMPLEX.\n\
             ROUTINE = simple, factual, quick (e.g. time, date, definitions).\n\
             COMPLEX = creative, long-form, reasoning (e.g. essays, poems, analysis).\n\n\
             User request: {}\n\nYour classification:",
            user_message
        );
        let request = CompletionRequest {
            messages: vec![Message {
                role: "user".into(),
                content: prompt,
            }],
        };
        let response = self.id.complete(&request).await?;
        let content = response.content.to_uppercase();
        let decision = if content.contains("COMPLEX") {
            RouteDecision::Complex
        } else {
            RouteDecision::Routine
        };
        tracing::info!("Routing decision: {:?} for input (len={})", decision, user_message.len());
        Ok(decision)
    }

    /// Route message: Id classifies; COMPLEX goes to Ego if configured, else Id.
    pub async fn route(&self, messages: Vec<Message>) -> anyhow::Result<CompletionResponse> {
        let last = messages
            .last()
            .map(|m| m.content.as_str())
            .unwrap_or("");
        let decision = self.classify(last).await?;

        let use_ego = matches!(decision, RouteDecision::Complex) && self.ego.is_some();
        if use_ego {
            tracing::info!("Routing to Ego (cloud)");
            let request = CompletionRequest { messages };
            self.ego.as_ref().unwrap().complete(&request).await
        } else {
            tracing::info!("Routing to Id (local)");
            let request = CompletionRequest { messages };
            self.id.complete(&request).await
        }
    }

    /// Privacy-sensitive: always use Id (local), never Ego.
    pub async fn id_only(&self, messages: Vec<Message>) -> anyhow::Result<CompletionResponse> {
        tracing::info!("id_only: using Id (local) only");
        let request = CompletionRequest { messages };
        self.id.complete(&request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_routing_decision() {
        let router = IdEgoRouter::new(None);
        let r = router.classify("What time is it?").await.unwrap();
        assert_eq!(r, RouteDecision::Routine);

        let r = router.classify("Write an essay on quantum mechanics.").await.unwrap();
        assert_eq!(r, RouteDecision::Complex);
    }
}
