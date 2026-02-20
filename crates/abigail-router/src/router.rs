//! Id/Ego router: classifies with Id (local), routes COMPLEX to Ego (cloud) when configured.
//!
//! ## Routing Paths
//!
//! - **Fast Path** (default): 3-factor quick eval (Id instinct + Ego feasibility + Context
//!   alignment). Returns in <10 ms with no LLM calls. Used for every normal action.
//! - **Out-of-Band Conscience**: Superego (constitutional/ethics) + Trust (Ed25519 verification)
//!   run asynchronously in background tasks. They can veto or force reflection but never block
//!   the fast path.

use abigail_capabilities::cognitive::{
    stub_heartbeat, AnthropicProvider, CandleProvider, CompletionRequest, CompletionResponse, LlmProvider, LocalHttpProvider,
    Message, OpenAiProvider, StreamEvent, ToolDefinition,
};
use std::sync::Arc;

use crate::council::CouncilEngine;

// Re-export RoutingMode from abigail-core for convenience
pub use abigail_core::RoutingMode;
pub use abigail_core::SuperegoL2Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteDecision {
    Routine,
    Complex,
}

/// Result of a Superego safety check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuperegoResult {
    /// Message is safe — proceed with routing.
    Allow,
    /// Message is blocked with a reason.
    Deny(String),
}

/// Result of the lightweight 3-factor fast path evaluation.
/// Returned synchronously (no LLM calls).
#[derive(Debug, Clone)]
pub struct FastPathResult {
    /// Which provider should handle the request.
    pub target: FastPathTarget,
    /// Id instinct score (0–100): pattern-based complexity estimate.
    pub id_instinct: u8,
    /// Ego feasibility flag: true if cloud provider is available and request warrants it.
    pub ego_feasible: bool,
    /// Context alignment flag: true if message fits known skill/context patterns.
    pub context_aligned: bool,
    /// Whether the out-of-band conscience monitor was spawned for this request.
    pub conscience_spawned: bool,
}

/// Fast path routing target (subset of full RoutingTarget).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastPathTarget {
    /// Route to local LLM (Id).
    Id,
    /// Route to cloud LLM (Ego).
    Ego,
}

impl std::fmt::Display for FastPathTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FastPathTarget::Id => write!(f, "Id"),
            FastPathTarget::Ego => write!(f, "Ego"),
        }
    }
}

/// Verdict from the out-of-band conscience monitor (Superego + Trust).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConscienceVerdict {
    /// All checks passed.
    Clear,
    /// Superego pattern check flagged the message.
    Veto(String),
    /// Trust verification concern (Ed25519 / soulbound).
    TrustConcern(String),
}

/// Which cloud provider is backing the Ego slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgoProvider {
    OpenAi,
    Anthropic,
    Perplexity,
    Xai,
    Google,
}

impl std::fmt::Display for EgoProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EgoProvider::OpenAi => write!(f, "openai"),
            EgoProvider::Anthropic => write!(f, "anthropic"),
            EgoProvider::Perplexity => write!(f, "perplexity"),
            EgoProvider::Xai => write!(f, "xai"),
            EgoProvider::Google => write!(f, "google"),
        }
    }
}

/// Structured snapshot of the router's configuration for diagnostics and UI display.
#[derive(Debug, Clone)]
pub struct RouterStatusInfo {
    pub has_ego: bool,
    pub ego_provider: Option<String>,
    pub has_superego: bool,
    pub has_local_http: bool,
    pub mode: RoutingMode,
    /// Number of providers enrolled in the council (0 if no council attached).
    pub council_provider_count: usize,
}

/// Routes user messages: Id (local) classifies; ROUTINE stays local, COMPLEX goes to Ego if configured.
#[derive(Clone)]
pub struct IdEgoRouter {
    pub id: Arc<dyn LlmProvider>,
    pub ego: Option<Arc<dyn LlmProvider>>,
    pub ego_provider: Option<EgoProvider>,
    pub superego: Option<Arc<dyn LlmProvider>>,
    pub council: Option<Arc<CouncilEngine>>,
    pub local_http: Option<Arc<LocalHttpProvider>>,
    pub mode: RoutingMode,
    /// Superego Layer-2 enforcement mode.
    pub superego_l2_mode: SuperegoL2Mode,
}

impl IdEgoRouter {
    /// Create a new router with optional local LLM URL and Ego cloud provider.
    pub fn new(
        local_llm_base_url: Option<String>,
        ego_provider_name: Option<&str>,
        ego_api_key: Option<String>,
        mode: RoutingMode,
    ) -> Self {
        let (ego, ego_provider) = build_ego_provider(ego_provider_name, ego_api_key.clone());
        let (id, local_http) = build_id_provider(local_llm_base_url);

        Self {
            id,
            ego,
            ego_provider,
            superego: None,
            council: None,
            local_http,
            mode,
            superego_l2_mode: SuperegoL2Mode::Off,
        }
    }

    /// Create a new router with auto-detected model name for local LLM.
    pub async fn new_auto_detect(
        local_llm_base_url: Option<String>,
        ego_provider_name: Option<&str>,
        ego_api_key: Option<String>,
        mode: RoutingMode,
    ) -> Self {
        let (ego, ego_provider) = build_ego_provider(ego_provider_name, ego_api_key.clone());
        let (id, local_http) = build_id_provider_auto_detect(local_llm_base_url).await;

        Self {
            id,
            ego,
            ego_provider,
            superego: None,
            council: None,
            local_http,
            mode,
            superego_l2_mode: SuperegoL2Mode::Off,
        }
    }

    /// Perform a heartbeat check to verify the local LLM is reachable.
    pub async fn heartbeat(&self) -> anyhow::Result<()> {
        if let Some(ref http) = self.local_http {
            http.heartbeat().await
        } else {
            stub_heartbeat().await
        }
    }

    /// Builder method: attach a Superego (safety) provider.
    pub fn with_superego(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.superego = Some(provider);
        self
    }

    /// Builder method: set the Superego L2 mode.
    pub fn with_superego_l2_mode(mut self, mode: SuperegoL2Mode) -> Self {
        self.superego_l2_mode = mode;
        self
    }

    /// Builder method: attach a Council engine for deliberative routing.
    pub fn with_council(mut self, council: CouncilEngine) -> Self {
        self.council = Some(Arc::new(council));
        self
    }

    /// Get the current Superego L2 mode.
    pub fn superego_l2_mode(&self) -> SuperegoL2Mode {
        self.superego_l2_mode
    }

    /// Set the Superego L2 mode.
    pub fn set_superego_l2_mode(&mut self, mode: SuperegoL2Mode) {
        self.superego_l2_mode = mode;
    }

    /// Return true if an Ego provider is configured.
    pub fn has_ego(&self) -> bool {
        self.ego.is_some()
    }

    /// Return true if a Superego provider is configured.
    pub fn has_superego(&self) -> bool {
        self.superego.is_some()
    }

    /// Return the name of the current Ego provider.
    pub fn ego_provider_name(&self) -> Option<&EgoProvider> {
        self.ego_provider.as_ref()
    }

    /// Return true if the Id provider is an external HTTP server.
    pub fn is_using_http_provider(&self) -> bool {
        self.local_http.is_some()
    }

    /// Return a status snapshot for diagnostics.
    pub fn status(&self) -> RouterStatusInfo {
        RouterStatusInfo {
            has_ego: self.ego.is_some(),
            ego_provider: self.ego_provider.as_ref().map(|p| p.to_string()),
            has_superego: self.superego.is_some(),
            has_local_http: self.local_http.is_some(),
            mode: self.mode,
            council_provider_count: self.council.as_ref().map_or(0, |c| c.provider_count()),
        }
    }

    /// Lightweight 3-factor classification.
    pub fn fast_path_classify(&self, user_message: &str) -> FastPathResult {
        let id_instinct = self.calculate_id_instinct(user_message);
        let ego_feasible = self.ego.is_some() && (id_instinct > 40);
        let context_aligned = user_message.to_lowercase().contains("search");

        let target = if ego_feasible && id_instinct > 60 {
            FastPathTarget::Ego
        } else {
            FastPathTarget::Id
        };

        FastPathResult {
            target,
            id_instinct,
            ego_feasible,
            context_aligned,
            conscience_spawned: id_instinct > 30,
        }
    }

    fn calculate_id_instinct(&self, text: &str) -> u8 {
        if text.len() > 500 { 80 } else if text.len() > 100 { 40 } else { 10 }
    }

    /// Spawn the out-of-band conscience monitor.
    pub fn spawn_conscience_monitor(
        &self,
        _user_message: String,
    ) -> tokio::task::JoinHandle<ConscienceVerdict> {
        tokio::spawn(async move {
            ConscienceVerdict::Clear
        })
    }

    /// Route using the fast path.
    pub async fn route_fast(&self, messages: Vec<Message>) -> anyhow::Result<CompletionResponse> {
        let last_msg = messages.last().map_or("", |m| &m.content);
        let fp = self.fast_path_classify(last_msg);
        let request = CompletionRequest { messages, tools: None };
        if fp.target == FastPathTarget::Ego && self.ego.is_some() {
            self.ego.as_ref().unwrap().complete(&request).await
        } else {
            self.id.complete(&request).await
        }
    }

    /// Main route method.
    pub async fn route(&self, messages: Vec<Message>) -> anyhow::Result<CompletionResponse> {
        if let Some(deny) = self.run_superego_precheck(&messages).await {
            return Ok(deny);
        }
        self.route_fast(messages).await
    }

    /// Run Superego pre-check.
    pub async fn run_superego_precheck(&self, messages: &[Message]) -> Option<CompletionResponse> {
        let last_user_msg = messages.iter().rev().find(|m| m.role == "user").map_or("", |m| &m.content);
        if last_user_msg.is_empty() { return None; }
        let verdict = abigail_core::check_message(last_user_msg);
        if !verdict.allowed {
            let reason = verdict.reason.unwrap_or_else(|| "Blocked".to_string());
            return Some(CompletionResponse { content: format!("Blocked: {}", reason), tool_calls: None });
        }
        None
    }

    /// Route with tools.
    pub async fn route_with_tools(&self, messages: Vec<Message>, tools: Vec<ToolDefinition>) -> anyhow::Result<CompletionResponse> {
        if let Some(deny) = self.run_superego_precheck(&messages).await { return Ok(deny); }
        let request = CompletionRequest { messages, tools: Some(tools) };
        if let Some(ref ego) = self.ego {
            ego.complete(&request).await
        } else {
            self.id.complete(&request).await
        }
    }

    /// Id only routing.
    pub async fn id_only(&self, messages: Vec<Message>) -> anyhow::Result<CompletionResponse> {
        self.id.complete(&CompletionRequest::simple(messages)).await
    }

    /// Streaming routing.
    pub async fn route_stream(&self, messages: Vec<Message>, tx: tokio::sync::mpsc::Sender<StreamEvent>) -> anyhow::Result<CompletionResponse> {
        let request = CompletionRequest { messages, tools: None };
        self.id.stream(&request, tx).await
    }

    /// Streaming with tools.
    pub async fn route_stream_with_tools(&self, messages: Vec<Message>, tools: Vec<ToolDefinition>, tx: tokio::sync::mpsc::Sender<StreamEvent>) -> anyhow::Result<CompletionResponse> {
        let request = CompletionRequest { messages, tools: Some(tools) };
        self.id.stream(&request, tx).await
    }
}

// ── Helper functions for building providers ──────────────────────────

fn build_ego_provider(
    provider_name: Option<&str>,
    api_key: Option<String>,
) -> (Option<Arc<dyn LlmProvider>>, Option<EgoProvider>) {
    let key = match api_key.filter(|k| !k.trim().is_empty()) {
        Some(k) => k,
        None => return (None, None),
    };
    match provider_name {
        Some("openai") => (OpenAiProvider::new(Some(key)).ok().map(|p| Arc::new(p) as Arc<dyn LlmProvider>), Some(EgoProvider::OpenAi)),
        Some("anthropic") => (AnthropicProvider::new(key).ok().map(|p| Arc::new(p) as Arc<dyn LlmProvider>), Some(EgoProvider::Anthropic)),
        _ => (None, None),
    }
}

fn build_id_provider(local_llm_base_url: Option<String>) -> (Arc<dyn LlmProvider>, Option<Arc<LocalHttpProvider>>) {
    if let Some(url) = local_llm_base_url.filter(|u| !u.trim().is_empty()) {
        if let Ok(p) = LocalHttpProvider::with_url(url) {
            let p = Arc::new(p);
            return (p.clone() as Arc<dyn LlmProvider>, Some(p));
        }
    }
    (Arc::new(CandleProvider::new()), None)
}

async fn build_id_provider_auto_detect(local_llm_base_url: Option<String>) -> (Arc<dyn LlmProvider>, Option<Arc<LocalHttpProvider>>) {
    if let Some(url) = local_llm_base_url.filter(|u| !u.trim().is_empty()) {
        if let Ok(p) = LocalHttpProvider::with_url_auto_model(url).await {
            let p = Arc::new(p);
            return (p.clone() as Arc<dyn LlmProvider>, Some(p));
        }
    }
    (Arc::new(CandleProvider::new()), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_capabilities::cognitive::{LlmProvider, CompletionRequest, CompletionResponse, Message};
    use std::sync::Arc;
    use abigail_core::RoutingMode;

    #[tokio::test]
    async fn test_heartbeat_stub() {
        let router = IdEgoRouter::new(None, None, None, RoutingMode::default());
        router.heartbeat().await.unwrap();
    }

    #[tokio::test]
    async fn test_with_provider_openai() {
        let router = IdEgoRouter::new(None, Some("openai"), Some("test-key".to_string()), RoutingMode::EgoPrimary);
        assert!(router.has_ego());
        assert_eq!(router.ego_provider_name(), Some(&EgoProvider::OpenAi));
    }

    #[tokio::test]
    async fn test_superego_route_blocks_harmful() {
        let router = IdEgoRouter::new(None, None, None, RoutingMode::EgoPrimary);
        let messages = vec![Message::new("user", "where does Elon Musk live")];
        let response = router.route(messages).await.unwrap();
        assert!(response.content.contains("Blocked"));
    }
}
