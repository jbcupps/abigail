//! Id/Ego router: classifies with Id (local), routes COMPLEX to Ego (cloud) when configured.

use ao_capabilities::cognitive::{
    stub_heartbeat, AnthropicProvider, CandleProvider, CompletionRequest, CompletionResponse,
    LocalHttpProvider, LlmProvider, Message, OpenAiProvider, StreamEvent, ToolDefinition,
};
use std::sync::Arc;

// Re-export RoutingMode from ao-core for convenience
pub use ao_core::RoutingMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteDecision {
    Routine,
    Complex,
}

/// Which cloud provider is backing the Ego slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgoProvider {
    OpenAi,
    Anthropic,
}

impl std::fmt::Display for EgoProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EgoProvider::OpenAi => write!(f, "openai"),
            EgoProvider::Anthropic => write!(f, "anthropic"),
        }
    }
}

/// Routes user messages: Id (local) classifies; ROUTINE stays local, COMPLEX goes to Ego if configured.
///
/// Id can be either:
/// - A local HTTP provider (LiteLLM, Ollama, etc.) when `local_llm_base_url` is set
/// - An in-process Candle stub when no URL is configured
///
/// Ego can be any cloud LLM provider implementing the LlmProvider trait.
#[derive(Clone)]
pub struct IdEgoRouter {
    id: Arc<dyn LlmProvider>,
    ego: Option<Arc<dyn LlmProvider>>,
    ego_provider: Option<EgoProvider>,
    local_http: Option<Arc<LocalHttpProvider>>,
    mode: RoutingMode,
}

impl IdEgoRouter {
    /// Create a new router with optional local LLM URL and OpenAI API key.
    /// This is the backward-compatible constructor (defaults Ego to OpenAI).
    ///
    /// # Arguments
    /// * `local_llm_base_url` - Base URL for local LLM server (e.g. "http://localhost:1234")
    /// * `openai_api_key` - OpenAI API key for Ego (cloud) routing
    /// * `mode` - Routing mode (EgoPrimary or IdPrimary)
    pub fn new(
        local_llm_base_url: Option<String>,
        openai_api_key: Option<String>,
        mode: RoutingMode,
    ) -> Self {
        let (ego, ego_provider): (Option<Arc<dyn LlmProvider>>, Option<EgoProvider>) =
            match openai_api_key.filter(|k| !k.is_empty()) {
                Some(k) => (
                    Some(Arc::new(OpenAiProvider::new(Some(k)))),
                    Some(EgoProvider::OpenAi),
                ),
                None => (None, None),
            };

        let (id, local_http) = build_id_provider(local_llm_base_url);

        Self {
            id,
            ego,
            ego_provider,
            local_http,
            mode,
        }
    }

    /// Create a new router with a specific cloud provider for Ego.
    ///
    /// # Arguments
    /// * `local_llm_base_url` - Base URL for local LLM server
    /// * `ego_provider_name` - Provider name: "openai", "anthropic"
    /// * `ego_api_key` - API key for the chosen provider
    /// * `mode` - Routing mode (EgoPrimary or IdPrimary)
    pub fn with_provider(
        local_llm_base_url: Option<String>,
        ego_provider_name: Option<&str>,
        ego_api_key: Option<String>,
        mode: RoutingMode,
    ) -> Self {
        let (ego, ego_provider) = build_ego_provider(ego_provider_name, ego_api_key);
        let (id, local_http) = build_id_provider(local_llm_base_url);

        Self {
            id,
            ego,
            ego_provider,
            local_http,
            mode,
        }
    }

    /// Create a new router with auto-detected model name for local LLM.
    /// This is the preferred constructor when a local LLM URL is provided.
    pub async fn new_auto_detect(
        local_llm_base_url: Option<String>,
        openai_api_key: Option<String>,
        mode: RoutingMode,
    ) -> Self {
        let (ego, ego_provider): (Option<Arc<dyn LlmProvider>>, Option<EgoProvider>) =
            match openai_api_key.filter(|k| !k.is_empty()) {
                Some(k) => (
                    Some(Arc::new(OpenAiProvider::new(Some(k)))),
                    Some(EgoProvider::OpenAi),
                ),
                None => (None, None),
            };

        let (id, local_http) = build_id_provider_auto_detect(local_llm_base_url).await;

        Self {
            id,
            ego,
            ego_provider,
            local_http,
            mode,
        }
    }

    /// Create a new router with auto-detected local LLM and a specific cloud provider.
    pub async fn with_provider_auto_detect(
        local_llm_base_url: Option<String>,
        ego_provider_name: Option<&str>,
        ego_api_key: Option<String>,
        mode: RoutingMode,
    ) -> Self {
        let (ego, ego_provider) = build_ego_provider(ego_provider_name, ego_api_key);
        let (id, local_http) = build_id_provider_auto_detect(local_llm_base_url).await;

        Self {
            id,
            ego,
            ego_provider,
            local_http,
            mode,
        }
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

    /// Check if Ego (cloud) is configured.
    pub fn has_ego(&self) -> bool {
        self.ego.is_some()
    }

    /// Get the name of the Ego provider, if configured.
    pub fn ego_provider_name(&self) -> Option<&EgoProvider> {
        self.ego_provider.as_ref()
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
        let request = CompletionRequest::simple(vec![Message::new("user", prompt)]);
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

    /// Route message based on configured routing mode.
    pub async fn route(&self, messages: Vec<Message>) -> anyhow::Result<CompletionResponse> {
        match self.mode {
            RoutingMode::IdPrimary => self.route_id_primary(messages).await,
            RoutingMode::EgoPrimary => self.route_ego_primary(messages).await,
        }
    }

    /// Id-primary routing: Id classifies; COMPLEX goes to Ego if configured, else Id.
    async fn route_id_primary(&self, messages: Vec<Message>) -> anyhow::Result<CompletionResponse> {
        let last = messages.last().map(|m| m.content.as_str()).unwrap_or("");
        let decision = self.classify(last).await?;

        let use_ego = matches!(decision, RouteDecision::Complex) && self.ego.is_some();
        if use_ego {
            tracing::info!("Routing to Ego (cloud) - complex request");
            let request = CompletionRequest { messages, tools: None };
            self.ego.as_ref().unwrap().complete(&request).await
        } else {
            tracing::info!("Routing to Id (local) - routine request");
            let request = CompletionRequest { messages, tools: None };
            self.id.complete(&request).await
        }
    }

    /// Ego-primary routing: Try Ego first if configured, fall back to Id on failure.
    async fn route_ego_primary(&self, messages: Vec<Message>) -> anyhow::Result<CompletionResponse> {
        // Try Ego first if configured
        if let Some(ego) = &self.ego {
            match ego
                .complete(&CompletionRequest {
                    messages: messages.clone(),
                    tools: None,
                })
                .await
            {
                Ok(response) => {
                    tracing::info!("Routed to Ego (cloud) - success");
                    return Ok(response);
                }
                Err(e) => {
                    tracing::warn!("Ego failed, falling back to Id: {}", e);
                }
            }
        }

        // Fallback to Id (local)
        tracing::info!("Routing to Id (local fallback)");
        self.id.complete(&CompletionRequest { messages, tools: None }).await
    }

    /// Route message with tool definitions attached.
    pub async fn route_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> anyhow::Result<CompletionResponse> {
        let request = CompletionRequest {
            messages,
            tools: Some(tools),
        };
        // For tool-calling, use Ego if available (better tool support), else Id.
        if let Some(ego) = &self.ego {
            match ego.complete(&request).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    tracing::warn!("Ego failed for tool call, falling back to Id: {}", e);
                }
            }
        }
        self.id.complete(&request).await
    }

    /// Privacy-sensitive: always use Id (local), never Ego.
    pub async fn id_only(&self, messages: Vec<Message>) -> anyhow::Result<CompletionResponse> {
        tracing::info!("id_only: using Id (local) only");
        let request = CompletionRequest::simple(messages);
        self.id.complete(&request).await
    }

    /// Streaming version of route(). Sends token events through the channel.
    /// Uses the same routing logic as route() but calls stream() instead of complete().
    pub async fn route_stream(
        &self,
        messages: Vec<Message>,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<CompletionResponse> {
        // Determine which provider to use (same logic as route)
        let provider: &Arc<dyn LlmProvider> = match self.mode {
            RoutingMode::EgoPrimary => {
                if let Some(ref ego) = self.ego {
                    ego
                } else {
                    &self.id
                }
            }
            RoutingMode::IdPrimary => {
                let last = messages.last().map(|m| m.content.as_str()).unwrap_or("");
                let decision = self.classify(last).await?;
                if matches!(decision, RouteDecision::Complex) {
                    if let Some(ref ego) = self.ego {
                        ego
                    } else {
                        &self.id
                    }
                } else {
                    &self.id
                }
            }
        };

        let request = CompletionRequest { messages, tools: None };
        provider.stream(&request, tx).await
    }

    /// Streaming version of route_with_tools().
    pub async fn route_stream_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<CompletionResponse> {
        let request = CompletionRequest {
            messages,
            tools: Some(tools),
        };
        // For tool-calling, prefer Ego if available
        if let Some(ref ego) = self.ego {
            match ego.stream(&request, tx).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    tracing::warn!("Ego stream failed for tool call, falling back to complete: {}", e);
                    // Fall back to non-streaming complete
                    return self.id.complete(&request).await;
                }
            }
        }
        self.id.complete(&request).await
    }
}

// ── Helper functions for building providers ──────────────────────────

/// Build the Ego (cloud) provider from a provider name and API key.
fn build_ego_provider(
    provider_name: Option<&str>,
    api_key: Option<String>,
) -> (Option<Arc<dyn LlmProvider>>, Option<EgoProvider>) {
    let key = match api_key.filter(|k| !k.is_empty()) {
        Some(k) => k,
        None => return (None, None),
    };

    match provider_name {
        Some("anthropic") => (
            Some(Arc::new(AnthropicProvider::new(key)) as Arc<dyn LlmProvider>),
            Some(EgoProvider::Anthropic),
        ),
        Some("openai") | None => (
            Some(Arc::new(OpenAiProvider::new(Some(key))) as Arc<dyn LlmProvider>),
            Some(EgoProvider::OpenAi),
        ),
        Some(unknown) => {
            tracing::warn!("Unknown ego provider '{}', falling back to OpenAI", unknown);
            (
                Some(Arc::new(OpenAiProvider::new(Some(key))) as Arc<dyn LlmProvider>),
                Some(EgoProvider::OpenAi),
            )
        }
    }
}

/// Build the Id (local) provider synchronously.
fn build_id_provider(
    local_llm_base_url: Option<String>,
) -> (Arc<dyn LlmProvider>, Option<Arc<LocalHttpProvider>>) {
    match local_llm_base_url.filter(|u| !u.is_empty()) {
        Some(url) => {
            let provider = Arc::new(LocalHttpProvider::with_url(url));
            (provider.clone() as Arc<dyn LlmProvider>, Some(provider))
        }
        None => (Arc::new(CandleProvider::new()) as Arc<dyn LlmProvider>, None),
    }
}

/// Build the Id (local) provider with auto-detected model name.
async fn build_id_provider_auto_detect(
    local_llm_base_url: Option<String>,
) -> (Arc<dyn LlmProvider>, Option<Arc<LocalHttpProvider>>) {
    match local_llm_base_url.filter(|u| !u.is_empty()) {
        Some(url) => {
            let provider = Arc::new(LocalHttpProvider::with_url_auto_model(url).await);
            (provider.clone() as Arc<dyn LlmProvider>, Some(provider))
        }
        None => (Arc::new(CandleProvider::new()) as Arc<dyn LlmProvider>, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_routing_decision() {
        // Use stub (no URL) for tests with id_primary mode
        let router = IdEgoRouter::new(None, None, RoutingMode::IdPrimary);
        let r = router.classify("What time is it?").await.unwrap();
        assert_eq!(r, RouteDecision::Routine);

        let r = router
            .classify("Write an essay on quantum mechanics.")
            .await
            .unwrap();
        assert_eq!(r, RouteDecision::Complex);
    }

    #[tokio::test]
    async fn test_heartbeat_stub() {
        let router = IdEgoRouter::new(None, None, RoutingMode::default());
        assert!(!router.is_using_http_provider());
        router.heartbeat().await.unwrap();
    }

    #[tokio::test]
    async fn test_default_routing_mode_is_ego_primary() {
        assert_eq!(RoutingMode::default(), RoutingMode::EgoPrimary);
    }

    #[tokio::test]
    async fn test_with_provider_anthropic() {
        let router = IdEgoRouter::with_provider(
            None,
            Some("anthropic"),
            Some("test-key".to_string()),
            RoutingMode::EgoPrimary,
        );
        assert!(router.has_ego());
        assert_eq!(router.ego_provider_name(), Some(&EgoProvider::Anthropic));
    }

    #[tokio::test]
    async fn test_with_provider_openai() {
        let router = IdEgoRouter::with_provider(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            RoutingMode::EgoPrimary,
        );
        assert!(router.has_ego());
        assert_eq!(router.ego_provider_name(), Some(&EgoProvider::OpenAi));
    }

    #[tokio::test]
    async fn test_with_provider_none_key() {
        let router = IdEgoRouter::with_provider(
            None,
            Some("anthropic"),
            None,
            RoutingMode::EgoPrimary,
        );
        assert!(!router.has_ego());
        assert_eq!(router.ego_provider_name(), None);
    }

    #[tokio::test]
    async fn test_backward_compat_new() {
        // Existing code calling new() should still work and default to OpenAI
        let router = IdEgoRouter::new(None, Some("test-key".to_string()), RoutingMode::EgoPrimary);
        assert!(router.has_ego());
        assert_eq!(router.ego_provider_name(), Some(&EgoProvider::OpenAi));
    }
}
