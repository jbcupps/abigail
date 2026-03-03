//! Id/Ego router: routes user messages to Ego (cloud/CLI) with Id (local) as failsafe.
//!
//! ## Routing Modes
//!
//! - **EgoPrimary**: All user-facing requests go to Ego; Id is failsafe only.
//! - **CliOrchestrator**: Auto-detected when Ego is a CLI provider (Claude CLI, etc.).

use abigail_capabilities::cognitive::{
    stub_heartbeat, CompletionRequest, CompletionResponse, LlmProvider, LocalHttpProvider, Message,
    StreamEvent, ToolDefinition,
};
use abigail_hive::{BuiltProviders, ProviderKind, ProviderRegistry};
use entity_core::{ExecutionTrace, SelectionReason};
use std::sync::Arc;

// Re-export RoutingMode from abigail-core for convenience
pub use abigail_core::RoutingMode;

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
    ClaudeCli,
    GeminiCli,
    CodexCli,
    GrokCli,
}

impl std::fmt::Display for EgoProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EgoProvider::OpenAi => write!(f, "openai"),
            EgoProvider::Anthropic => write!(f, "anthropic"),
            EgoProvider::Perplexity => write!(f, "perplexity"),
            EgoProvider::Xai => write!(f, "xai"),
            EgoProvider::Google => write!(f, "google"),
            EgoProvider::ClaudeCli => write!(f, "claude-cli"),
            EgoProvider::GeminiCli => write!(f, "gemini-cli"),
            EgoProvider::CodexCli => write!(f, "codex-cli"),
            EgoProvider::GrokCli => write!(f, "grok-cli"),
        }
    }
}

impl From<ProviderKind> for EgoProvider {
    fn from(kind: ProviderKind) -> Self {
        match kind {
            ProviderKind::OpenAi => EgoProvider::OpenAi,
            ProviderKind::Anthropic => EgoProvider::Anthropic,
            ProviderKind::Perplexity => EgoProvider::Perplexity,
            ProviderKind::Xai => EgoProvider::Xai,
            ProviderKind::Google => EgoProvider::Google,
            ProviderKind::ClaudeCli => EgoProvider::ClaudeCli,
            ProviderKind::GeminiCli => EgoProvider::GeminiCli,
            ProviderKind::CodexCli => EgoProvider::CodexCli,
            ProviderKind::GrokCli => EgoProvider::GrokCli,
        }
    }
}

// ── Unified routing types ────────────────────────────────────────

/// All parameters for a single routing call, replacing 10+ individual method signatures.
pub struct RoutingRequest {
    /// Messages to send (system + history + user).
    pub messages: Vec<Message>,
    /// Tool definitions (None = no tool-use).
    pub tools: Option<Vec<ToolDefinition>>,
    /// Explicit model override from the caller.
    pub model_override: Option<String>,
    /// Streaming channel (None = non-streaming).
    pub stream_tx: Option<tokio::sync::mpsc::Sender<StreamEvent>>,
    /// Force Id-only routing (skip Ego target selection).
    pub force_id_only: bool,
}

impl RoutingRequest {
    /// Simple non-streaming, non-tool request.
    pub fn simple(messages: Vec<Message>) -> Self {
        Self {
            messages,
            tools: None,
            model_override: None,
            stream_tx: None,
            force_id_only: false,
        }
    }

    /// Request with tools.
    pub fn with_tools(messages: Vec<Message>, tools: Vec<ToolDefinition>) -> Self {
        Self {
            messages,
            tools: Some(tools),
            model_override: None,
            stream_tx: None,
            force_id_only: false,
        }
    }
}

/// Unified response from any routing call.
pub struct RoutingResponse {
    /// The completion from the provider.
    pub completion: CompletionResponse,
    /// Execution trace (always present from `route_unified`).
    pub trace: Option<ExecutionTrace>,
}

/// Structured snapshot of the router's configuration for diagnostics and UI display.
#[derive(Debug, Clone)]
pub struct RouterStatusInfo {
    pub has_ego: bool,
    pub ego_provider: Option<String>,
    pub has_local_http: bool,
    pub mode: RoutingMode,
}

/// Read-only diagnosis of what the router would do for a given message,
/// without actually calling any LLM.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoutingDiagnosis {
    pub mode: String,
    pub target: String,
    pub selected_model: Option<String>,
    pub selection_reason: String,
    pub ego_provider: Option<String>,
    pub has_local_llm: bool,
}

/// Routes user messages via EgoPrimary or CliOrchestrator mode.
///
/// Ego (cloud/CLI provider) handles all user-facing requests; Id (local LLM)
/// serves as a failsafe when the cloud provider fails.
#[derive(Clone)]
pub struct IdEgoRouter {
    pub id: Arc<dyn LlmProvider>,
    pub ego: Option<Arc<dyn LlmProvider>>,
    pub ego_provider: Option<EgoProvider>,
    pub local_http: Option<Arc<LocalHttpProvider>>,
    pub mode: RoutingMode,
}

impl IdEgoRouter {
    /// Chooses Id vs Ego for the main routing path. Chat and direct user prompts
    /// always use Ego when available; Id is reserved for background tasks
    /// (memory, cron) invoked explicitly via `id_only()`.
    fn target_for_mode(&self, _user_message: &str) -> FastPathTarget {
        if self.ego.is_some() {
            return FastPathTarget::Ego;
        }
        FastPathTarget::Id
    }
    /// Create a new router with optional local LLM URL and Ego cloud provider.
    pub fn new(
        local_llm_base_url: Option<String>,
        ego_provider_name: Option<&str>,
        ego_api_key: Option<String>,
        ego_model: Option<String>,
        mode: RoutingMode,
    ) -> Self {
        let ego_result = ProviderRegistry::build_ego(ego_provider_name, ego_api_key, ego_model);
        let id_result = ProviderRegistry::build_id(local_llm_base_url);

        Self {
            id: id_result.provider,
            ego: ego_result.provider,
            ego_provider: ego_result.kind.map(EgoProvider::from),
            local_http: id_result.local_http,
            mode,
        }
    }

    /// Create a new router with auto-detected model name for local LLM.
    pub async fn new_auto_detect(
        local_llm_base_url: Option<String>,
        ego_provider_name: Option<&str>,
        ego_api_key: Option<String>,
        ego_model: Option<String>,
        mode: RoutingMode,
    ) -> Self {
        let ego_result = ProviderRegistry::build_ego(ego_provider_name, ego_api_key, ego_model);
        let id_result = ProviderRegistry::build_id_auto_detect(local_llm_base_url).await;

        Self {
            id: id_result.provider,
            ego: ego_result.provider,
            ego_provider: ego_result.kind.map(EgoProvider::from),
            local_http: id_result.local_http,
            mode,
        }
    }

    /// Create a router from pre-built providers (constructed by the Hive).
    pub fn from_built_providers(providers: BuiltProviders) -> Self {
        let ego_provider = providers.ego_kind.map(EgoProvider::from);

        // Auto-upgrade to CliOrchestrator when the ego is a CLI variant.
        let mode = match (&ego_provider, providers.routing_mode) {
            (
                Some(
                    EgoProvider::ClaudeCli
                    | EgoProvider::GeminiCli
                    | EgoProvider::CodexCli
                    | EgoProvider::GrokCli,
                ),
                RoutingMode::EgoPrimary,
            ) => {
                tracing::info!(
                    "Auto-upgrading routing mode to CliOrchestrator (ego provider is {:?})",
                    ego_provider
                );
                RoutingMode::CliOrchestrator
            }
            (_, mode) => mode,
        };

        Self {
            id: providers.id,
            ego: providers.ego,
            ego_provider,
            local_http: providers.local_http,
            mode,
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

    /// Return true if an Ego provider is configured.
    pub fn has_ego(&self) -> bool {
        self.ego.is_some()
    }

    /// Return the name of the current Ego provider.
    pub fn ego_provider_name(&self) -> Option<&EgoProvider> {
        self.ego_provider.as_ref()
    }

    /// Return true if the Id provider is an external HTTP server.
    pub fn is_using_http_provider(&self) -> bool {
        self.local_http.is_some()
    }

    /// Get the best available provider for simple completion (no routing).
    ///
    /// Returns Ego if available, then local HTTP, then `None`. This is used
    /// by the birth pipeline which needs a single provider reference rather
    /// than the full routing surface.
    pub fn best_available_provider(&self) -> Option<Arc<dyn LlmProvider>> {
        if let Some(ref ego) = self.ego {
            Some(ego.clone())
        } else {
            self.local_http
                .as_ref()
                .map(|lh| lh.clone() as Arc<dyn LlmProvider>)
        }
    }

    /// Return a status snapshot for diagnostics.
    pub fn status(&self) -> RouterStatusInfo {
        RouterStatusInfo {
            has_ego: self.ego.is_some(),
            ego_provider: self.ego_provider.as_ref().map(|p| p.to_string()),
            has_local_http: self.local_http.is_some(),
            mode: self.mode,
        }
    }

    /// Diagnose what the router would do for a given message without calling any LLM.
    pub fn diagnose(&self, user_message: &str) -> RoutingDiagnosis {
        let target = self.target_for_mode(user_message);
        let target_str = match target {
            FastPathTarget::Ego => "ego",
            FastPathTarget::Id => "id",
        };

        let selection_reason = match self.mode {
            RoutingMode::CliOrchestrator => "cli_orchestrator".to_string(),
            _ => "ego_primary".to_string(),
        };

        RoutingDiagnosis {
            mode: format!("{:?}", self.mode).to_lowercase(),
            target: target_str.to_string(),
            selected_model: None,
            selection_reason,
            ego_provider: self.ego_provider.as_ref().map(|p| p.to_string()),
            has_local_llm: self.local_http.is_some(),
        }
    }

    // ── Classification ──────────────────────────────────────────────

    /// Lightweight classification: routes to Ego when available, Id otherwise.
    pub fn fast_path_classify(&self, user_message: &str) -> FastPathResult {
        let context_aligned = self.has_external_context_signal(user_message);
        let ego_feasible = self.ego.is_some();
        let target = if ego_feasible {
            FastPathTarget::Ego
        } else {
            FastPathTarget::Id
        };

        FastPathResult {
            target,
            id_instinct: 0,
            ego_feasible,
            context_aligned,
            conscience_spawned: false,
        }
    }

    fn has_external_context_signal(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        [
            "search",
            "web",
            "http",
            "api",
            "fetch",
            "docs",
            "documentation",
            "latest",
            "current",
            "today",
            "news",
            "url",
            "crawl",
        ]
        .iter()
        .any(|k| lower.contains(k))
    }

    /// Spawn the out-of-band conscience monitor.
    pub fn spawn_conscience_monitor(
        &self,
        _user_message: String,
    ) -> tokio::task::JoinHandle<ConscienceVerdict> {
        tokio::spawn(async move { ConscienceVerdict::Clear })
    }

    // ── Unified routing ──────────────────────────────────────────────

    /// Unified routing entry point that replaces 10+ individual methods.
    ///
    /// Routes to Ego (cloud/CLI) with Id fallback, supports streaming and tools.
    pub async fn route_unified(&self, req: RoutingRequest) -> anyhow::Result<RoutingResponse> {
        let last_msg = req.messages.last().map_or("", |m| &m.content).to_string();

        let (target, model_override) = if req.force_id_only {
            (FastPathTarget::Id, None)
        } else {
            let target = self.target_for_mode(&last_msg);
            (target, req.model_override)
        };

        let mut trace = self.begin_trace(&last_msg, &model_override);

        let request = CompletionRequest {
            messages: req.messages,
            tools: req.tools,
            model_override: model_override.clone(),
        };

        let completion = self
            .execute_with_fallback(&request, target, &model_override, req.stream_tx, &mut trace)
            .await?;

        Ok(RoutingResponse {
            completion,
            trace: Some(trace),
        })
    }

    /// Internal fallback chain: try primary target, fall back to the other on failure.
    async fn execute_with_fallback(
        &self,
        request: &CompletionRequest,
        target: FastPathTarget,
        model_override: &Option<String>,
        stream_tx: Option<tokio::sync::mpsc::Sender<StreamEvent>>,
        trace: &mut ExecutionTrace,
    ) -> anyhow::Result<CompletionResponse> {
        let is_stream = stream_tx.is_some();

        if target == FastPathTarget::Ego {
            if let Some(ref ego) = self.ego {
                let t0 = chrono::Utc::now();
                let result = if let Some(ref tx) = stream_tx {
                    ego.stream(request, tx.clone()).await
                } else {
                    ego.complete(request).await
                };
                match result {
                    Ok(response) => {
                        trace.record_success(&self.ego_label(), model_override.clone(), t0);
                        return Ok(response);
                    }
                    Err(e) => {
                        trace.record_error(
                            &self.ego_label(),
                            model_override.clone(),
                            &e.to_string(),
                            t0,
                        );
                        tracing::warn!(
                            "Ego provider failed{}, falling back to Id: {}",
                            if is_stream { " (stream)" } else { "" },
                            e
                        );
                        let t1 = chrono::Utc::now();
                        let resp = if let Some(ref tx) = stream_tx {
                            self.id.stream(request, tx.clone()).await?
                        } else {
                            self.id.complete(request).await?
                        };
                        trace.record_success(self.id_label(), None, t1);
                        return Ok(resp);
                    }
                }
            }
        }

        // Id-first path (or no Ego available)
        let t0 = chrono::Utc::now();
        let result = if let Some(ref tx) = stream_tx {
            self.id.stream(request, tx.clone()).await
        } else {
            self.id.complete(request).await
        };
        match result {
            Ok(response) => {
                trace.record_success(self.id_label(), None, t0);
                Ok(response)
            }
            Err(e) => {
                trace.record_error(self.id_label(), None, &e.to_string(), t0);
                if let Some(ref ego) = self.ego {
                    tracing::warn!(
                        "Id provider failed{}, falling back to Ego: {}",
                        if is_stream { " (stream)" } else { "" },
                        e
                    );
                    let t1 = chrono::Utc::now();
                    let resp = if let Some(ref tx) = stream_tx {
                        ego.stream(request, tx.clone()).await?
                    } else {
                        ego.complete(request).await?
                    };
                    trace.record_success(&self.ego_label(), model_override.clone(), t1);
                    return Ok(resp);
                }
                Err(e)
            }
        }
    }

    // ── Legacy routing methods (thin wrappers over route_unified) ────

    /// Route using the fast path.
    #[deprecated(since = "0.4.0", note = "Use route_unified() instead")]
    pub async fn route_fast(&self, messages: Vec<Message>) -> anyhow::Result<CompletionResponse> {
        let last_msg = messages.last().map_or("", |m| &m.content);
        let target = self.target_for_mode(last_msg);
        let request = CompletionRequest {
            messages,
            tools: None,
            model_override: None,
        };
        if target == FastPathTarget::Ego {
            if let Some(ref ego) = self.ego {
                match ego.complete(&request).await {
                    Ok(response) => return Ok(response),
                    Err(e) => {
                        tracing::warn!("Ego provider failed, falling back to Id: {}", e);
                        return self.id.complete(&request).await;
                    }
                }
            }
        }
        match self.id.complete(&request).await {
            Ok(response) => Ok(response),
            Err(e) => {
                if let Some(ref ego) = self.ego {
                    tracing::warn!("Id provider failed, falling back to Ego: {}", e);
                    return ego.complete(&request).await;
                }
                Err(e)
            }
        }
    }

    /// Main route method.
    #[deprecated(since = "0.4.0", note = "Use route_unified() instead")]
    pub async fn route(&self, messages: Vec<Message>) -> anyhow::Result<CompletionResponse> {
        #[allow(deprecated)]
        self.route_fast(messages).await
    }

    /// Route with tools.
    #[deprecated(since = "0.4.0", note = "Use route_unified() instead")]
    pub async fn route_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> anyhow::Result<CompletionResponse> {
        let last_msg = messages.last().map_or("", |m| &m.content);
        let target = self.target_for_mode(last_msg);
        let request = CompletionRequest {
            messages,
            tools: Some(tools),
            model_override: None,
        };
        if target == FastPathTarget::Ego {
            if let Some(ref ego) = self.ego {
                match ego.complete(&request).await {
                    Ok(response) => return Ok(response),
                    Err(e) => {
                        tracing::warn!(
                            "Ego provider failed (with tools), falling back to Id: {}",
                            e
                        );
                        return self.id.complete(&request).await;
                    }
                }
            }
        }
        match self.id.complete(&request).await {
            Ok(response) => Ok(response),
            Err(e) => {
                if let Some(ref ego) = self.ego {
                    tracing::warn!(
                        "Id provider failed (with tools), falling back to Ego: {}",
                        e
                    );
                    return ego.complete(&request).await;
                }
                Err(e)
            }
        }
    }

    /// Id only routing (falls back to Ego when Id fails).
    #[deprecated(
        since = "0.4.0",
        note = "Use route_unified() with force_id_only instead"
    )]
    pub async fn id_only(&self, messages: Vec<Message>) -> anyhow::Result<CompletionResponse> {
        let request = CompletionRequest::simple(messages);
        match self.id.complete(&request).await {
            Ok(response) => Ok(response),
            Err(e) => {
                if let Some(ref ego) = self.ego {
                    tracing::warn!("Id-only failed, falling back to Ego: {}", e);
                    return ego.complete(&request).await;
                }
                Err(e)
            }
        }
    }

    /// Id only streaming (falls back to Ego when Id fails).
    #[deprecated(
        since = "0.4.0",
        note = "Use route_unified() with force_id_only + stream_tx instead"
    )]
    pub async fn id_stream(
        &self,
        messages: Vec<Message>,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<CompletionResponse> {
        let request = CompletionRequest::simple(messages);
        match self.id.stream(&request, tx.clone()).await {
            Ok(response) => Ok(response),
            Err(e) => {
                if let Some(ref ego) = self.ego {
                    tracing::warn!("Id-only stream failed, falling back to Ego: {}", e);
                    return ego.stream(&request, tx).await;
                }
                Err(e)
            }
        }
    }

    /// Streaming routing with failsafe.
    #[deprecated(since = "0.4.0", note = "Use route_unified() with stream_tx instead")]
    pub async fn route_stream(
        &self,
        messages: Vec<Message>,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<CompletionResponse> {
        tracing::debug!(
            "Routing stream: mode={:?}, has_ego={}",
            self.mode,
            self.ego.is_some()
        );
        let last_msg = messages.last().map_or("", |m| &m.content);
        let target = self.target_for_mode(last_msg);
        let request = CompletionRequest {
            messages,
            tools: None,
            model_override: None,
        };
        if target == FastPathTarget::Ego {
            if let Some(ref ego) = self.ego {
                tracing::debug!("Routing stream to Ego (mode/fast-path)");
                match ego.stream(&request, tx.clone()).await {
                    Ok(response) => return Ok(response),
                    Err(e) => {
                        tracing::warn!("Ego stream failed, falling back to Id: {}", e);
                        return self.id.stream(&request, tx).await;
                    }
                }
            }
        }
        tracing::debug!("Routing stream to Id (mode/fast-path target: {:?})", target);
        match self.id.stream(&request, tx.clone()).await {
            Ok(response) => Ok(response),
            Err(e) => {
                if let Some(ref ego) = self.ego {
                    tracing::warn!("Id stream failed, falling back to Ego: {}", e);
                    return ego.stream(&request, tx).await;
                }
                Err(e)
            }
        }
    }

    /// Streaming with tools.
    #[deprecated(
        since = "0.4.0",
        note = "Use route_unified() with tools + stream_tx instead"
    )]
    pub async fn route_stream_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<CompletionResponse> {
        tracing::debug!(
            "Routing stream with tools: mode={:?}, has_ego={}",
            self.mode,
            self.ego.is_some()
        );
        let last_msg = messages.last().map_or("", |m| &m.content);
        let target = self.target_for_mode(last_msg);
        let request = CompletionRequest {
            messages,
            tools: Some(tools),
            model_override: None,
        };
        if target == FastPathTarget::Ego {
            if let Some(ref ego) = self.ego {
                tracing::debug!("Routing to Ego (mode/fast-path)");
                match ego.stream(&request, tx.clone()).await {
                    Ok(response) => return Ok(response),
                    Err(e) => {
                        tracing::warn!("Ego stream with tools failed, falling back to Id: {}", e);
                        return self.id.stream(&request, tx).await;
                    }
                }
            }
        }
        tracing::debug!("Routing to Id (mode/fast-path target: {:?})", target);
        match self.id.stream(&request, tx.clone()).await {
            Ok(response) => Ok(response),
            Err(e) => {
                if let Some(ref ego) = self.ego {
                    tracing::warn!("Id stream with tools failed, falling back to Ego: {}", e);
                    return ego.stream(&request, tx).await;
                }
                Err(e)
            }
        }
    }

    // ── Traced routing methods ────────────────────────────────────────
    //
    // Each `*_traced` variant returns `(CompletionResponse, ExecutionTrace)`.
    // The trace captures configured intent, actual execution path, timing,
    // and fallback chain — making it the single source of truth for
    // per-turn attribution.

    fn id_label(&self) -> &str {
        if self.local_http.is_some() {
            "id(local_http)"
        } else {
            "id(candle_stub)"
        }
    }

    fn ego_label(&self) -> String {
        self.ego_provider
            .as_ref()
            .map(|p| p.to_string())
            .unwrap_or_else(|| "ego".to_string())
    }

    fn routing_mode_str(&self) -> String {
        format!("{:?}", self.mode).to_lowercase()
    }

    /// Build a fresh trace pre-populated with routing intent for a user message.
    pub fn begin_trace(
        &self,
        user_message: &str,
        model_override: &Option<String>,
    ) -> ExecutionTrace {
        let target = self.target_for_mode(user_message);
        let target_str = match target {
            FastPathTarget::Ego => "ego",
            FastPathTarget::Id => "id",
        };
        let mut trace = ExecutionTrace::new(
            &self.routing_mode_str(),
            self.ego_provider.as_ref().map(|p| p.to_string()),
            model_override.clone(),
            target_str,
        );

        trace.selection_reason = Some(SelectionReason::EgoPrimary);

        trace
    }

    /// Traced variant of `route_fast` / `route`.
    #[deprecated(
        since = "0.4.0",
        note = "Use route_unified() instead — it always returns a trace"
    )]
    pub async fn route_traced(
        &self,
        messages: Vec<Message>,
    ) -> anyhow::Result<(CompletionResponse, ExecutionTrace)> {
        let last_msg = messages.last().map_or("", |m| &m.content).to_string();
        let target = self.target_for_mode(&last_msg);
        let model_override: Option<String> = None;
        let mut trace = self.begin_trace(&last_msg, &model_override);
        let request = CompletionRequest {
            messages,
            tools: None,
            model_override: None,
        };

        if target == FastPathTarget::Ego {
            if let Some(ref ego) = self.ego {
                let t0 = chrono::Utc::now();
                match ego.complete(&request).await {
                    Ok(response) => {
                        trace.record_success(&self.ego_label(), model_override, t0);
                        return Ok((response, trace));
                    }
                    Err(e) => {
                        trace.record_error(
                            &self.ego_label(),
                            model_override.clone(),
                            &e.to_string(),
                            t0,
                        );
                        tracing::warn!("Ego provider failed, falling back to Id: {}", e);
                        let t1 = chrono::Utc::now();
                        let resp = self.id.complete(&request).await?;
                        trace.record_success(self.id_label(), None, t1);
                        return Ok((resp, trace));
                    }
                }
            }
        }

        let t0 = chrono::Utc::now();
        match self.id.complete(&request).await {
            Ok(response) => {
                trace.record_success(self.id_label(), None, t0);
                Ok((response, trace))
            }
            Err(e) => {
                trace.record_error(self.id_label(), None, &e.to_string(), t0);
                if let Some(ref ego) = self.ego {
                    tracing::warn!("Id provider failed, falling back to Ego: {}", e);
                    let t1 = chrono::Utc::now();
                    let resp = ego.complete(&request).await?;
                    trace.record_success(&self.ego_label(), model_override, t1);
                    return Ok((resp, trace));
                }
                Err(e)
            }
        }
    }

    /// Traced variant of `route_with_tools`.
    #[deprecated(since = "0.4.0", note = "Use route_unified() with tools instead")]
    pub async fn route_with_tools_traced(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> anyhow::Result<(CompletionResponse, ExecutionTrace)> {
        #[allow(deprecated)]
        self.route_with_tools_traced_override(messages, tools, None)
            .await
    }

    /// Traced routing with tools and an optional explicit model override.
    #[deprecated(
        since = "0.4.0",
        note = "Use route_unified() with tools + model_override instead"
    )]
    pub async fn route_with_tools_traced_override(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        forced_model_override: Option<String>,
    ) -> anyhow::Result<(CompletionResponse, ExecutionTrace)> {
        let last_msg = messages.last().map_or("", |m| &m.content).to_string();
        let target = self.target_for_mode(&last_msg);
        let model_override = forced_model_override;
        let mut trace = self.begin_trace(&last_msg, &model_override);
        let request = CompletionRequest {
            messages,
            tools: Some(tools),
            model_override: model_override.clone(),
        };

        if target == FastPathTarget::Ego {
            if let Some(ref ego) = self.ego {
                let t0 = chrono::Utc::now();
                match ego.complete(&request).await {
                    Ok(response) => {
                        trace.record_success(&self.ego_label(), model_override, t0);
                        return Ok((response, trace));
                    }
                    Err(e) => {
                        trace.record_error(
                            &self.ego_label(),
                            model_override.clone(),
                            &e.to_string(),
                            t0,
                        );
                        tracing::warn!(
                            "Ego provider failed (with tools), falling back to Id: {}",
                            e
                        );
                        let t1 = chrono::Utc::now();
                        let resp = self.id.complete(&request).await?;
                        trace.record_success(self.id_label(), None, t1);
                        return Ok((resp, trace));
                    }
                }
            }
        }

        let t0 = chrono::Utc::now();
        match self.id.complete(&request).await {
            Ok(response) => {
                trace.record_success(self.id_label(), None, t0);
                Ok((response, trace))
            }
            Err(e) => {
                trace.record_error(self.id_label(), None, &e.to_string(), t0);
                if let Some(ref ego) = self.ego {
                    tracing::warn!(
                        "Id provider failed (with tools), falling back to Ego: {}",
                        e
                    );
                    let t1 = chrono::Utc::now();
                    let resp = ego.complete(&request).await?;
                    trace.record_success(&self.ego_label(), model_override, t1);
                    return Ok((resp, trace));
                }
                Err(e)
            }
        }
    }

    /// Traced variant of `id_only`.
    #[deprecated(
        since = "0.4.0",
        note = "Use route_unified() with force_id_only instead"
    )]
    pub async fn id_only_traced(
        &self,
        messages: Vec<Message>,
    ) -> anyhow::Result<(CompletionResponse, ExecutionTrace)> {
        let mut trace = ExecutionTrace::new(
            &self.routing_mode_str(),
            self.ego_provider.as_ref().map(|p| p.to_string()),
            None,
            "id",
        );
        let request = CompletionRequest::simple(messages);
        let t0 = chrono::Utc::now();
        match self.id.complete(&request).await {
            Ok(response) => {
                trace.record_success(self.id_label(), None, t0);
                Ok((response, trace))
            }
            Err(e) => {
                trace.record_error(self.id_label(), None, &e.to_string(), t0);
                if let Some(ref ego) = self.ego {
                    tracing::warn!("Id-only failed, falling back to Ego: {}", e);
                    let t1 = chrono::Utc::now();
                    let resp = ego.complete(&request).await?;
                    trace.record_success(&self.ego_label(), None, t1);
                    return Ok((resp, trace));
                }
                Err(e)
            }
        }
    }

    /// Traced variant of `id_stream`.
    #[deprecated(
        since = "0.4.0",
        note = "Use route_unified() with force_id_only + stream_tx instead"
    )]
    pub async fn id_stream_traced(
        &self,
        messages: Vec<Message>,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<(CompletionResponse, ExecutionTrace)> {
        let mut trace = ExecutionTrace::new(
            &self.routing_mode_str(),
            self.ego_provider.as_ref().map(|p| p.to_string()),
            None,
            "id",
        );
        let request = CompletionRequest::simple(messages);
        let t0 = chrono::Utc::now();
        match self.id.stream(&request, tx.clone()).await {
            Ok(response) => {
                trace.record_success(self.id_label(), None, t0);
                Ok((response, trace))
            }
            Err(e) => {
                trace.record_error(self.id_label(), None, &e.to_string(), t0);
                if let Some(ref ego) = self.ego {
                    tracing::warn!("Id-only stream failed, falling back to Ego: {}", e);
                    let t1 = chrono::Utc::now();
                    let resp = ego.stream(&request, tx).await?;
                    trace.record_success(&self.ego_label(), None, t1);
                    return Ok((resp, trace));
                }
                Err(e)
            }
        }
    }

    /// Traced variant of `route_stream`.
    #[deprecated(since = "0.4.0", note = "Use route_unified() with stream_tx instead")]
    pub async fn route_stream_traced(
        &self,
        messages: Vec<Message>,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<(CompletionResponse, ExecutionTrace)> {
        let last_msg = messages.last().map_or("", |m| &m.content).to_string();
        let target = self.target_for_mode(&last_msg);
        let model_override: Option<String> = None;
        let mut trace = self.begin_trace(&last_msg, &model_override);
        let request = CompletionRequest {
            messages,
            tools: None,
            model_override: model_override.clone(),
        };

        if target == FastPathTarget::Ego {
            if let Some(ref ego) = self.ego {
                let t0 = chrono::Utc::now();
                match ego.stream(&request, tx.clone()).await {
                    Ok(response) => {
                        trace.record_success(&self.ego_label(), model_override, t0);
                        return Ok((response, trace));
                    }
                    Err(e) => {
                        trace.record_error(
                            &self.ego_label(),
                            model_override.clone(),
                            &e.to_string(),
                            t0,
                        );
                        tracing::warn!("Ego stream failed, falling back to Id: {}", e);
                        let t1 = chrono::Utc::now();
                        let resp = self.id.stream(&request, tx).await?;
                        trace.record_success(self.id_label(), None, t1);
                        return Ok((resp, trace));
                    }
                }
            }
        }

        let t0 = chrono::Utc::now();
        match self.id.stream(&request, tx.clone()).await {
            Ok(response) => {
                trace.record_success(self.id_label(), None, t0);
                Ok((response, trace))
            }
            Err(e) => {
                trace.record_error(self.id_label(), None, &e.to_string(), t0);
                if let Some(ref ego) = self.ego {
                    tracing::warn!("Id stream failed, falling back to Ego: {}", e);
                    let t1 = chrono::Utc::now();
                    let resp = ego.stream(&request, tx).await?;
                    trace.record_success(&self.ego_label(), model_override, t1);
                    return Ok((resp, trace));
                }
                Err(e)
            }
        }
    }

    /// Traced variant of `route_stream_with_tools`.
    #[deprecated(
        since = "0.4.0",
        note = "Use route_unified() with tools + stream_tx instead"
    )]
    pub async fn route_stream_with_tools_traced(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<(CompletionResponse, ExecutionTrace)> {
        let last_msg = messages.last().map_or("", |m| &m.content).to_string();
        let target = self.target_for_mode(&last_msg);
        let model_override: Option<String> = None;
        let mut trace = self.begin_trace(&last_msg, &model_override);
        let request = CompletionRequest {
            messages,
            tools: Some(tools),
            model_override: model_override.clone(),
        };

        if target == FastPathTarget::Ego {
            if let Some(ref ego) = self.ego {
                let t0 = chrono::Utc::now();
                match ego.stream(&request, tx.clone()).await {
                    Ok(response) => {
                        trace.record_success(&self.ego_label(), model_override, t0);
                        return Ok((response, trace));
                    }
                    Err(e) => {
                        trace.record_error(
                            &self.ego_label(),
                            model_override.clone(),
                            &e.to_string(),
                            t0,
                        );
                        tracing::warn!("Ego stream with tools failed, falling back to Id: {}", e);
                        let t1 = chrono::Utc::now();
                        let resp = self.id.stream(&request, tx).await?;
                        trace.record_success(self.id_label(), None, t1);
                        return Ok((resp, trace));
                    }
                }
            }
        }

        let t0 = chrono::Utc::now();
        match self.id.stream(&request, tx.clone()).await {
            Ok(response) => {
                trace.record_success(self.id_label(), None, t0);
                Ok((response, trace))
            }
            Err(e) => {
                trace.record_error(self.id_label(), None, &e.to_string(), t0);
                if let Some(ref ego) = self.ego {
                    tracing::warn!("Id stream with tools failed, falling back to Ego: {}", e);
                    let t1 = chrono::Utc::now();
                    let resp = ego.stream(&request, tx).await?;
                    trace.record_success(&self.ego_label(), model_override, t1);
                    return Ok((resp, trace));
                }
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_capabilities::cognitive::Message;
    use abigail_core::RoutingMode;

    #[tokio::test]
    async fn test_heartbeat_stub() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::default());
        router.heartbeat().await.unwrap();
    }

    #[tokio::test]
    async fn test_with_provider_openai() {
        let router = IdEgoRouter::new(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            None,
            RoutingMode::EgoPrimary,
        );
        assert!(router.has_ego());
        assert_eq!(router.ego_provider_name(), Some(&EgoProvider::OpenAi));
    }

    #[tokio::test]
    async fn test_ego_primary_without_ego_falls_to_id() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::EgoPrimary);
        let target = router.target_for_mode("this is a complex question that might use ego");
        assert_eq!(target, FastPathTarget::Id);
    }

    #[tokio::test]
    async fn test_fast_path_classify_prefers_ego_for_external_context() {
        let router = IdEgoRouter::new(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            None,
            RoutingMode::EgoPrimary,
        );
        let fp =
            router.fast_path_classify("Search the web for the latest incident response guidance");
        assert_eq!(fp.target, FastPathTarget::Ego);
        assert!(fp.context_aligned);
    }

    #[tokio::test]
    async fn test_fast_path_classify_routes_to_ego_when_available() {
        let router = IdEgoRouter::new(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            None,
            RoutingMode::EgoPrimary,
        );
        let fp = router.fast_path_classify("hi");
        assert_eq!(fp.target, FastPathTarget::Ego);
    }

    #[tokio::test]
    async fn test_route_with_tools_uses_failsafe() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::EgoPrimary);
        #[allow(deprecated)]
        let response = router
            .route_with_tools(
                vec![Message::new("user", "hello")],
                vec![abigail_capabilities::cognitive::ToolDefinition {
                    name: "test_tool".to_string(),
                    description: "test".to_string(),
                    parameters: serde_json::json!({ "type": "object" }),
                }],
            )
            .await
            .unwrap();
        assert!(!response.content.is_empty());
    }

    // ── Execution trace tests ─────────────────────────────────────

    #[test]
    fn test_begin_trace_captures_routing_intent() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::EgoPrimary);
        let trace = router.begin_trace("hello", &None);
        assert_eq!(trace.routing_mode, "egoprimary");
        assert_eq!(trace.target_selected, "id");
        assert!(trace.configured_provider.is_none());
        assert!(trace.configured_model.is_none());
        assert!(trace.steps.is_empty());
        assert!(!trace.fallback_occurred);
    }

    #[test]
    fn test_trace_record_success_no_fallback() {
        let mut trace = ExecutionTrace::new(
            "egoprimary",
            Some("openai".into()),
            Some("gpt-4.1-mini".into()),
            "ego",
        );
        let t0 = chrono::Utc::now();
        trace.record_success("openai", Some("gpt-4.1-mini".into()), t0);
        assert_eq!(trace.steps.len(), 1);
        assert_eq!(trace.final_step_index, 0);
        assert!(!trace.fallback_occurred);
        assert_eq!(trace.final_provider(), Some("openai"));
        assert_eq!(trace.final_model(), Some("gpt-4.1-mini"));
    }

    #[test]
    fn test_trace_record_error_then_fallback() {
        let mut trace = ExecutionTrace::new("egoprimary", Some("openai".into()), None, "ego");
        let t0 = chrono::Utc::now();
        trace.record_error("openai", Some("gpt-4.1".into()), "timeout", t0);
        assert_eq!(trace.steps.len(), 1);
        assert_eq!(trace.steps[0].result, entity_core::StepResult::Error);

        let t1 = chrono::Utc::now();
        trace.record_success("id(candle_stub)", None, t1);
        assert_eq!(trace.steps.len(), 2);
        assert_eq!(trace.final_step_index, 1);
        assert!(trace.fallback_occurred);
        assert_eq!(trace.final_provider(), Some("id(candle_stub)"));
        assert_eq!(trace.final_model(), None);
    }

    #[tokio::test]
    async fn test_route_traced_id_only_success() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::EgoPrimary);
        let messages = vec![Message {
            role: "user".to_string(),
            content: "hello".to_string(),
            tool_call_id: None,
            tool_calls: None,
        }];
        #[allow(deprecated)]
        let (resp, trace) = router.route_traced(messages).await.unwrap();
        assert!(!resp.content.is_empty());
        assert_eq!(trace.target_selected, "id");
        assert!(!trace.fallback_occurred);
        assert_eq!(trace.steps.len(), 1);
        assert_eq!(trace.steps[0].result, entity_core::StepResult::Success);
    }

    #[tokio::test]
    async fn test_id_only_traced_success() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::EgoPrimary);
        let messages = vec![Message {
            role: "user".to_string(),
            content: "hello".to_string(),
            tool_call_id: None,
            tool_calls: None,
        }];
        #[allow(deprecated)]
        let (resp, trace) = router.id_only_traced(messages).await.unwrap();
        assert!(!resp.content.is_empty());
        assert_eq!(trace.target_selected, "id");
        assert!(!trace.fallback_occurred);
        assert!(trace.final_provider().is_some());
    }

    #[test]
    fn test_begin_trace_ego_primary_reason() {
        let router = IdEgoRouter::new(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            None,
            RoutingMode::EgoPrimary,
        );
        let trace = router.begin_trace("hi", &None);
        assert_eq!(trace.selection_reason, Some(SelectionReason::EgoPrimary));
    }

    // ── diagnose tests ────────────────────────────────────────────

    #[test]
    fn test_diagnose_ego_primary() {
        let router = IdEgoRouter::new(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            None,
            RoutingMode::EgoPrimary,
        );
        let diag = router.diagnose("hi");
        assert_eq!(diag.mode, "egoprimary");
        assert_eq!(diag.target, "ego");
        assert_eq!(diag.selection_reason, "ego_primary");
    }

    // ── Trace final_tier tests ────────────────────────────────────

    #[test]
    fn test_trace_final_tier_no_fallback() {
        let mut trace = ExecutionTrace::new("egoprimary", Some("openai".into()), None, "ego");
        trace.configured_tier = Some("fast".to_string());
        let t0 = chrono::Utc::now();
        trace.record_success("openai", Some("gpt-4.1-mini".into()), t0);
        assert_eq!(trace.final_tier(), Some("fast"));
    }

    #[test]
    fn test_trace_final_tier_fallback_clears_tier() {
        let mut trace = ExecutionTrace::new("egoprimary", Some("openai".into()), None, "ego");
        trace.configured_tier = Some("pro".to_string());
        let t0 = chrono::Utc::now();
        trace.record_error("openai", None, "timeout", t0);
        let t1 = chrono::Utc::now();
        trace.record_success("id(candle_stub)", None, t1);
        assert!(trace.fallback_occurred);
        assert_eq!(trace.final_tier(), None);
    }
}
