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
    stub_heartbeat, CompletionRequest, CompletionResponse, LlmProvider, LocalHttpProvider, Message,
    StreamEvent, ToolDefinition,
};
use abigail_core::{ForceOverride, ModelTier, TierModels, TierThresholds};
use abigail_hive::{BuiltProviders, ProviderKind, ProviderRegistry};
use entity_core::ExecutionTrace;
use std::sync::Arc;

use crate::council::CouncilEngine;

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

/// Structured snapshot of the router's configuration for diagnostics and UI display.
#[derive(Debug, Clone)]
pub struct RouterStatusInfo {
    pub has_ego: bool,
    pub ego_provider: Option<String>,
    pub has_local_http: bool,
    pub mode: RoutingMode,
    /// Number of providers enrolled in the council (0 if no council attached).
    pub council_provider_count: usize,
}

/// Routes user messages: Id (local) classifies; ROUTINE stays local, COMPLEX goes to Ego if configured.
///
/// In TierBased mode, the router uses complexity scoring to select a model tier
/// (Fast/Standard/Pro) and resolves the correct model via `tier_models`. Force
/// overrides can pin a specific tier or model. The Id (local LLM) is used as a
/// failsafe when the cloud provider fails.
#[derive(Clone)]
pub struct IdEgoRouter {
    pub id: Arc<dyn LlmProvider>,
    pub ego: Option<Arc<dyn LlmProvider>>,
    pub ego_provider: Option<EgoProvider>,
    pub council: Option<Arc<CouncilEngine>>,
    pub local_http: Option<Arc<LocalHttpProvider>>,
    pub mode: RoutingMode,
    /// Per-provider model assignments for each tier.
    pub tier_models: TierModels,
    /// Complexity score thresholds for tier selection.
    pub tier_thresholds: TierThresholds,
    /// Force override (pin tier, model, or provider+tier).
    pub force_override: ForceOverride,
}

impl IdEgoRouter {
    fn target_for_mode(&self, user_message: &str) -> FastPathTarget {
        match self.mode {
            RoutingMode::EgoPrimary => {
                if self.ego.is_some() {
                    FastPathTarget::Ego
                } else {
                    FastPathTarget::Id
                }
            }
            RoutingMode::Council | RoutingMode::TierBased => {
                self.fast_path_classify(user_message).target
            }
        }
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
            council: None,
            local_http: id_result.local_http,
            mode,
            tier_models: TierModels::defaults(),
            tier_thresholds: TierThresholds::default(),
            force_override: ForceOverride::default(),
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
            council: None,
            local_http: id_result.local_http,
            mode,
            tier_models: TierModels::defaults(),
            tier_thresholds: TierThresholds::default(),
            force_override: ForceOverride::default(),
        }
    }

    /// Create a router from pre-built providers (constructed by the Hive).
    pub fn from_built_providers(providers: BuiltProviders) -> Self {
        Self {
            id: providers.id,
            ego: providers.ego,
            ego_provider: providers.ego_kind.map(EgoProvider::from),
            council: None,
            local_http: providers.local_http,
            mode: providers.routing_mode,
            tier_models: providers.tier_models,
            tier_thresholds: providers.tier_thresholds,
            force_override: providers.force_override,
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

    /// Builder method: attach a Council engine for deliberative routing.
    pub fn with_council(mut self, council: CouncilEngine) -> Self {
        self.council = Some(Arc::new(council));
        self
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

    /// Return a status snapshot for diagnostics.
    pub fn status(&self) -> RouterStatusInfo {
        RouterStatusInfo {
            has_ego: self.ego.is_some(),
            ego_provider: self.ego_provider.as_ref().map(|p| p.to_string()),
            has_local_http: self.local_http.is_some(),
            mode: self.mode,
            council_provider_count: self.council.as_ref().map_or(0, |c| c.provider_count()),
        }
    }

    // ── Tier selection & model resolution ──────────────────────────

    /// Keywords that signal a setup, configuration, or credential-storage intent.
    const SETUP_INTENT_KEYWORDS: &'static [&'static str] = &[
        "set up",
        "setup",
        "configure",
        "configuration",
        "credential",
        "credentials",
        "imap",
        "smtp",
        "mailbox",
        "api key",
        "api_key",
        "password",
        "hostname",
        "connect my",
        "store secret",
        "store_secret",
        "account setup",
        "login details",
    ];

    /// Returns `true` if the message appears to be a setup / credential operation.
    pub fn detect_setup_intent(user_message: &str) -> bool {
        let lower = user_message.to_lowercase();
        Self::SETUP_INTENT_KEYWORDS
            .iter()
            .any(|kw| lower.contains(kw))
    }

    /// Select the model tier based on force override, setup intent, or complexity score.
    ///
    /// Priority:
    /// 1. `force_override.pinned_tier` — user pinned a tier
    /// 2. Setup / credential intent → Pro (auto-escalation)
    /// 3. Complexity score → `tier_thresholds.score_to_tier()`
    pub fn select_tier(&self, user_message: &str) -> ModelTier {
        if let Some(tier) = self.force_override.pinned_tier {
            tracing::debug!("Tier pinned by force override: {:?}", tier);
            return tier;
        }
        if Self::detect_setup_intent(user_message) {
            tracing::debug!("Setup/credential intent detected — escalating to Pro tier");
            return ModelTier::Pro;
        }
        let score = self.calculate_id_instinct(user_message);
        let tier = self.tier_thresholds.score_to_tier(score);
        tracing::debug!("Tier selected by complexity score {}: {:?}", score, tier);
        tier
    }

    /// Resolve the model to use for a request.
    ///
    /// Priority chain:
    /// 1. `force_override.pinned_model` — exact model pin (highest)
    /// 2. `force_override.pinned_tier` (+ optional `pinned_provider`) → tier_models lookup
    /// 3. Complexity-based tier → tier_models lookup for the active ego provider
    ///
    /// Returns `None` if no tier_models mapping exists for the resolved tier+provider.
    pub fn resolve_model_for_request(&self, user_message: &str) -> Option<String> {
        // 1. Pinned model — highest priority
        if let Some(ref model) = self.force_override.pinned_model {
            tracing::debug!("Model pinned by force override: {}", model);
            return Some(model.clone());
        }

        // Determine provider name for tier_models lookup
        let provider_name = self.force_override.pinned_provider.as_deref().or_else(|| {
            self.ego_provider.as_ref().map(|p| match p {
                EgoProvider::OpenAi => "openai",
                EgoProvider::Anthropic => "anthropic",
                EgoProvider::Perplexity => "perplexity",
                EgoProvider::Xai => "xai",
                EgoProvider::Google => "google",
                // CLI providers don't participate in tier model selection
                EgoProvider::ClaudeCli
                | EgoProvider::GeminiCli
                | EgoProvider::CodexCli
                | EgoProvider::GrokCli => "cli",
            })
        });

        let provider_name = match provider_name {
            Some(name) if name != "cli" => name,
            _ => {
                tracing::debug!("No provider for tier model lookup");
                return None;
            }
        };

        // 2/3. Select tier (pinned or complexity-based) and look up model
        let tier = self.select_tier(user_message);
        let model = self.tier_models.get_model(provider_name, tier).cloned();
        tracing::debug!(
            "Tier model resolved: provider={}, tier={:?}, model={:?}",
            provider_name,
            tier,
            model
        );
        model
    }

    /// Compute tier routing metadata for a message.
    ///
    /// Returns `(tier_name, model_id, complexity_score)` reflecting
    /// what `resolve_model_for_request` would produce. Useful for
    /// populating ChatResponse metadata without duplicating logic.
    pub fn tier_metadata_for_message(
        &self,
        user_message: &str,
    ) -> (Option<String>, Option<String>, Option<u8>) {
        if self.mode != RoutingMode::TierBased || self.ego.is_none() {
            return (None, None, None);
        }
        let score = self.calculate_id_instinct(user_message);
        let tier = if let Some(t) = self.force_override.pinned_tier {
            t
        } else {
            self.tier_thresholds.score_to_tier(score)
        };
        let model = self.resolve_model_for_request(user_message);
        let tier_name = match tier {
            ModelTier::Fast => "fast",
            ModelTier::Standard => "standard",
            ModelTier::Pro => "pro",
        };
        (Some(tier_name.to_string()), model, Some(score))
    }

    // ── Classification ──────────────────────────────────────────────

    /// Lightweight 3-factor classification.
    pub fn fast_path_classify(&self, user_message: &str) -> FastPathResult {
        let id_instinct = self.calculate_id_instinct(user_message);
        let context_aligned = self.has_external_context_signal(user_message);
        let ego_feasible = self.ego.is_some() && (id_instinct >= 45 || context_aligned);

        let target =
            if ego_feasible && (id_instinct >= 60 || (context_aligned && id_instinct >= 20)) {
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
        let lower = text.to_lowercase();
        let mut score: i32 = match text.len() {
            n if n > 1200 => 90,
            n if n > 600 => 75,
            n if n > 250 => 55,
            n if n > 100 => 35,
            _ => 15,
        };

        let complexity_terms = [
            "analyze",
            "compare",
            "tradeoff",
            "design",
            "architecture",
            "debug",
            "investigate",
            "optimize",
            "benchmark",
            "route",
            "routing",
            "strategy",
            "security",
            "incident",
            "multiple",
            "step-by-step",
            "plan",
            "refactor",
        ];
        score += (complexity_terms
            .iter()
            .filter(|k| lower.contains(**k))
            .count() as i32)
            * 6;

        let has_structured_content =
            text.contains('\n') || text.contains("```") || text.contains("{") || text.contains("[");
        if has_structured_content {
            score += 10;
        }

        let question_count = text.chars().filter(|c| *c == '?').count() as i32;
        if question_count >= 2 {
            score += 8;
        }

        score.clamp(5, 95) as u8
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

    /// Route using the fast path with tier-based model selection.
    pub async fn route_fast(&self, messages: Vec<Message>) -> anyhow::Result<CompletionResponse> {
        let last_msg = messages.last().map_or("", |m| &m.content);
        let target = self.target_for_mode(last_msg);
        let model_override = if self.mode == RoutingMode::TierBased {
            self.resolve_model_for_request(last_msg)
        } else {
            None
        };
        let request = CompletionRequest {
            messages,
            tools: None,
            model_override,
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
    pub async fn route(&self, messages: Vec<Message>) -> anyhow::Result<CompletionResponse> {
        self.route_fast(messages).await
    }

    /// Route with tools and tier-based model selection.
    pub async fn route_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> anyhow::Result<CompletionResponse> {
        let last_msg = messages.last().map_or("", |m| &m.content);
        let target = self.target_for_mode(last_msg);
        let model_override = if self.mode == RoutingMode::TierBased {
            self.resolve_model_for_request(last_msg)
        } else {
            None
        };
        let request = CompletionRequest {
            messages,
            tools: Some(tools),
            model_override,
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

    /// Streaming routing with tier-based model selection and failsafe.
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
        let model_override = if self.mode == RoutingMode::TierBased {
            self.resolve_model_for_request(last_msg)
        } else {
            None
        };
        let request = CompletionRequest {
            messages,
            tools: None,
            model_override,
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

    /// Streaming with tools and tier-based model selection.
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
        let model_override = if self.mode == RoutingMode::TierBased {
            self.resolve_model_for_request(last_msg)
        } else {
            None
        };
        let request = CompletionRequest {
            messages,
            tools: Some(tools),
            model_override,
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
        ExecutionTrace::new(
            &self.routing_mode_str(),
            self.ego_provider.as_ref().map(|p| p.to_string()),
            model_override.clone(),
            target_str,
        )
    }

    /// Traced variant of `route_fast` / `route`.
    pub async fn route_traced(
        &self,
        messages: Vec<Message>,
    ) -> anyhow::Result<(CompletionResponse, ExecutionTrace)> {
        let last_msg = messages.last().map_or("", |m| &m.content).to_string();
        let target = self.target_for_mode(&last_msg);
        let model_override = if self.mode == RoutingMode::TierBased {
            self.resolve_model_for_request(&last_msg)
        } else {
            None
        };
        let mut trace = self.begin_trace(&last_msg, &model_override);
        let request = CompletionRequest {
            messages,
            tools: None,
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
    pub async fn route_with_tools_traced(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> anyhow::Result<(CompletionResponse, ExecutionTrace)> {
        let last_msg = messages.last().map_or("", |m| &m.content).to_string();
        let target = self.target_for_mode(&last_msg);
        let model_override = if self.mode == RoutingMode::TierBased {
            self.resolve_model_for_request(&last_msg)
        } else {
            None
        };
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
    pub async fn route_stream_traced(
        &self,
        messages: Vec<Message>,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<(CompletionResponse, ExecutionTrace)> {
        let last_msg = messages.last().map_or("", |m| &m.content).to_string();
        let target = self.target_for_mode(&last_msg);
        let model_override = if self.mode == RoutingMode::TierBased {
            self.resolve_model_for_request(&last_msg)
        } else {
            None
        };
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
    pub async fn route_stream_with_tools_traced(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<(CompletionResponse, ExecutionTrace)> {
        let last_msg = messages.last().map_or("", |m| &m.content).to_string();
        let target = self.target_for_mode(&last_msg);
        let model_override = if self.mode == RoutingMode::TierBased {
            self.resolve_model_for_request(&last_msg)
        } else {
            None
        };
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

    // ── Builder methods for tier configuration ──────────────────────

    /// Builder method: set tier models.
    pub fn with_tier_models(mut self, tier_models: TierModels) -> Self {
        self.tier_models = tier_models;
        self
    }

    /// Builder method: set tier thresholds.
    pub fn with_tier_thresholds(mut self, thresholds: TierThresholds) -> Self {
        self.tier_thresholds = thresholds;
        self
    }

    /// Builder method: set force override.
    pub fn with_force_override(mut self, force_override: ForceOverride) -> Self {
        self.force_override = force_override;
        self
    }

    /// Set the force override at runtime (e.g. from Forge UI).
    pub fn set_force_override(&mut self, force_override: ForceOverride) {
        self.force_override = force_override;
    }

    /// Set tier thresholds at runtime.
    pub fn set_tier_thresholds(&mut self, thresholds: TierThresholds) {
        self.tier_thresholds = thresholds;
    }

    /// Set tier models at runtime.
    pub fn set_tier_models(&mut self, tier_models: TierModels) {
        self.tier_models = tier_models;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_capabilities::cognitive::{Message, ToolDefinition};
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
            RoutingMode::TierBased,
        );
        let fp =
            router.fast_path_classify("Search the web for the latest incident response guidance");
        assert_eq!(fp.target, FastPathTarget::Ego);
        assert!(fp.context_aligned);
    }

    #[tokio::test]
    async fn test_fast_path_classify_keeps_short_local_message_on_id() {
        let router = IdEgoRouter::new(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            None,
            RoutingMode::TierBased,
        );
        let fp = router.fast_path_classify("hi");
        assert_eq!(fp.target, FastPathTarget::Id);
    }

    #[tokio::test]
    async fn test_route_with_tools_tier_based_uses_failsafe() {
        // TierBased with no providers configured falls through to the stub failsafe
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        let response = router
            .route_with_tools(
                vec![Message::new("user", "hello")],
                vec![ToolDefinition {
                    name: "test_tool".to_string(),
                    description: "test".to_string(),
                    parameters: serde_json::json!({ "type": "object" }),
                }],
            )
            .await
            .unwrap();
        assert!(!response.content.is_empty());
    }

    #[test]
    fn test_select_tier_default_thresholds() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        // Short simple message → low score → Fast
        let tier = router.select_tier("hi");
        assert_eq!(tier, ModelTier::Fast);
    }

    #[test]
    fn test_select_tier_complex_message() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        // Long complex message → high score → Pro
        let tier = router.select_tier(
            "Analyze the security architecture of this distributed system, compare the tradeoffs \
             between multiple approaches, and design a step-by-step plan to optimize the routing \
             strategy. Include benchmarks and investigate potential vulnerabilities.",
        );
        assert_eq!(tier, ModelTier::Pro);
    }

    #[test]
    fn test_select_tier_pinned_override() {
        let mut router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        router.force_override = ForceOverride {
            pinned_model: None,
            pinned_tier: Some(ModelTier::Pro),
            pinned_provider: None,
        };
        // Even a simple message should return Pro when tier is pinned
        let tier = router.select_tier("hi");
        assert_eq!(tier, ModelTier::Pro);
    }

    #[test]
    fn test_resolve_model_pinned_model() {
        let mut router = IdEgoRouter::new(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            None,
            RoutingMode::TierBased,
        );
        router.force_override = ForceOverride {
            pinned_model: Some("custom-model-v9".to_string()),
            pinned_tier: None,
            pinned_provider: None,
        };
        let model = router.resolve_model_for_request("anything");
        assert_eq!(model, Some("custom-model-v9".to_string()));
    }

    #[test]
    fn test_resolve_model_tier_based_for_openai() {
        let router = IdEgoRouter::new(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            None,
            RoutingMode::TierBased,
        );
        // Simple message → Fast tier → gpt-4.1-mini
        let model = router.resolve_model_for_request("hi");
        assert_eq!(model, Some("gpt-4.1-mini".to_string()));
    }

    #[test]
    fn test_resolve_model_no_ego_provider() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        // No ego provider → no model resolution
        let model = router.resolve_model_for_request("hi");
        assert_eq!(model, None);
    }

    #[test]
    fn test_builder_tier_config() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased)
            .with_tier_thresholds(TierThresholds {
                fast_ceiling: 20,
                pro_floor: 80,
            })
            .with_force_override(ForceOverride {
                pinned_tier: Some(ModelTier::Fast),
                pinned_model: None,
                pinned_provider: None,
            });
        assert_eq!(router.tier_thresholds.fast_ceiling, 20);
        assert_eq!(router.tier_thresholds.pro_floor, 80);
        assert_eq!(router.force_override.pinned_tier, Some(ModelTier::Fast));
    }

    #[test]
    fn test_set_force_override_runtime() {
        let mut router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        assert!(!router.force_override.is_active());
        router.set_force_override(ForceOverride {
            pinned_model: Some("test-model".to_string()),
            pinned_tier: None,
            pinned_provider: None,
        });
        assert!(router.force_override.is_active());
    }

    #[test]
    fn test_tier_metadata_no_ego() {
        // No ego → no tier metadata
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        let (tier, model, score) = router.tier_metadata_for_message("hello world");
        assert!(tier.is_none());
        assert!(model.is_none());
        assert!(score.is_none());
    }

    #[test]
    fn test_tier_metadata_ego_primary_mode() {
        // EgoPrimary mode → no tier metadata (only TierBased emits metadata)
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::EgoPrimary);
        let (tier, model, score) = router.tier_metadata_for_message("complex query");
        assert!(tier.is_none());
        assert!(model.is_none());
        assert!(score.is_none());
    }

    #[test]
    fn test_tier_metadata_tier_based_simple() {
        let router = IdEgoRouter::new(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            None,
            RoutingMode::TierBased,
        );

        // Simple message → should be Fast tier
        let (tier, model, score) = router.tier_metadata_for_message("hi");
        assert_eq!(tier.as_deref(), Some("fast"));
        assert_eq!(model, Some("gpt-4.1-mini".to_string())); // openai fast model
        assert!(score.is_some());
        assert!(score.unwrap() < 35); // Simple msg should be below fast_ceiling
    }

    // ── Setup intent detection ─────────────────────────────────────

    #[test]
    fn test_detect_setup_intent_positive() {
        assert!(IdEgoRouter::detect_setup_intent(
            "Please set up your email Mailbox details"
        ));
        assert!(IdEgoRouter::detect_setup_intent(
            "configure my IMAP credentials"
        ));
        assert!(IdEgoRouter::detect_setup_intent(
            "I need to store my API key"
        ));
        assert!(IdEgoRouter::detect_setup_intent("Setup SMTP with password"));
    }

    #[test]
    fn test_detect_setup_intent_negative() {
        assert!(!IdEgoRouter::detect_setup_intent("hi"));
        assert!(!IdEgoRouter::detect_setup_intent(
            "what is the weather in Miami"
        ));
        assert!(!IdEgoRouter::detect_setup_intent("tell me a joke"));
    }

    #[test]
    fn test_setup_intent_escalates_to_pro() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        let tier = router.select_tier("please set up my mailbox with IMAP credentials");
        assert_eq!(tier, ModelTier::Pro);
    }

    #[test]
    fn test_pinned_tier_overrides_setup_intent() {
        let mut router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        router.force_override = ForceOverride {
            pinned_model: None,
            pinned_tier: Some(ModelTier::Fast),
            pinned_provider: None,
        };
        let tier = router.select_tier("configure my IMAP credentials");
        assert_eq!(
            tier,
            ModelTier::Fast,
            "User pinned tier should take precedence over intent escalation"
        );
    }

    // ── Execution trace tests ─────────────────────────────────────

    #[test]
    fn test_begin_trace_captures_routing_intent() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        let trace = router.begin_trace("hello", &None);
        assert_eq!(trace.routing_mode, "tierbased");
        assert_eq!(trace.target_selected, "id");
        assert!(trace.configured_provider.is_none());
        assert!(trace.configured_model.is_none());
        assert!(trace.steps.is_empty());
        assert!(!trace.fallback_occurred);
    }

    #[test]
    fn test_trace_record_success_no_fallback() {
        let mut trace = ExecutionTrace::new(
            "tierbased",
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
        let mut trace = ExecutionTrace::new("tierbased", Some("openai".into()), None, "ego");
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
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        let messages = vec![Message {
            role: "user".to_string(),
            content: "hello".to_string(),
            tool_call_id: None,
            tool_calls: None,
        }];
        let (resp, trace) = router.route_traced(messages).await.unwrap();
        assert!(!resp.content.is_empty());
        assert_eq!(trace.target_selected, "id");
        assert!(!trace.fallback_occurred);
        assert_eq!(trace.steps.len(), 1);
        assert_eq!(trace.steps[0].result, entity_core::StepResult::Success);
    }

    #[tokio::test]
    async fn test_id_only_traced_success() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        let messages = vec![Message {
            role: "user".to_string(),
            content: "hello".to_string(),
            tool_call_id: None,
            tool_calls: None,
        }];
        let (resp, trace) = router.id_only_traced(messages).await.unwrap();
        assert!(!resp.content.is_empty());
        assert_eq!(trace.target_selected, "id");
        assert!(!trace.fallback_occurred);
        assert!(trace.final_provider().is_some());
    }
}
