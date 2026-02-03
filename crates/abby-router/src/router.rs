//! Id/Ego router: classifies with Id (local), routes COMPLEX to Ego (cloud) when configured.

use abby_llm::{
    stub_heartbeat, CandleProvider, CompletionRequest, CompletionResponse, LocalHttpProvider,
    LlmProvider, Message, OpenAiProvider,
};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteDecision {
    Routine,
    Complex,
}

/// Routes user messages: Id (local) classifies; ROUTINE stays local, COMPLEX goes to Ego if configured.
///
/// Id can be either:
/// - A local HTTP provider (LiteLLM, Ollama, etc.) when `local_llm_base_url` is set
/// - An in-process Candle stub when no URL is configured
pub struct IdEgoRouter {
    id: Arc<dyn LlmProvider>,
    ego: Option<Arc<OpenAiProvider>>,
    local_http: Option<Arc<LocalHttpProvider>>,
}

impl IdEgoRouter {
    /// Create a new router with optional local LLM URL and OpenAI API key.
    ///
    /// # Arguments
    /// * `local_llm_base_url` - Base URL for local LLM server (e.g. "http://localhost:1234")
    /// * `openai_api_key` - OpenAI API key for Ego (cloud) routing
    pub fn new(local_llm_base_url: Option<String>, openai_api_key: Option<String>) -> Self {
        let ego = openai_api_key
            .filter(|k| !k.is_empty())
            .map(|k| Arc::new(OpenAiProvider::new(Some(k))));

        let (id, local_http): (Arc<dyn LlmProvider>, Option<Arc<LocalHttpProvider>>) =
            match local_llm_base_url.filter(|u| !u.is_empty()) {
                Some(url) => {
                    let provider = Arc::new(LocalHttpProvider::with_url(url));
                    (provider.clone() as Arc<dyn LlmProvider>, Some(provider))
                }
                None => (Arc::new(CandleProvider::new()) as Arc<dyn LlmProvider>, None),
            };

        Self { id, ego, local_http }
    }

    /// Perform a heartbeat check to verify the local LLM is reachable.
    /// If using HTTP provider, sends a minimal request; if using stub, always succeeds.
    pub async fn heartbeat(&self) -> anyhow::Result<()> {
        match &self.local_http {
            Some(provider) => provider.heartbeat().await,
            None => stub_heartbeat().await,
        }
    }

    /// Check if using a local HTTP provider (vs in-process stub).
    pub fn is_using_http_provider(&self) -> bool {
        self.local_http.is_some()
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
        // Use stub (no URL) for tests
        let router = IdEgoRouter::new(None, None);
        let r = router.classify("What time is it?").await.unwrap();
        assert_eq!(r, RouteDecision::Routine);

        let r = router.classify("Write an essay on quantum mechanics.").await.unwrap();
        assert_eq!(r, RouteDecision::Complex);
    }

    #[tokio::test]
    async fn test_heartbeat_stub() {
        let router = IdEgoRouter::new(None, None);
        assert!(!router.is_using_http_provider());
        router.heartbeat().await.unwrap();
    }
}
