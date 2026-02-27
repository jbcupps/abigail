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
use entity_core::{ExecutionTrace, SelectionReason};
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

/// Read-only diagnosis of what the router would do for a given message,
/// without actually calling any LLM.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoutingDiagnosis {
    pub mode: String,
    pub target: String,
    pub selected_tier: Option<String>,
    pub selected_model: Option<String>,
    pub complexity_score: Option<u8>,
    pub selection_reason: String,
    pub ego_provider: Option<String>,
    pub has_local_llm: bool,
    pub council_available: bool,
    pub council_provider_count: usize,
    pub force_override_active: bool,
    pub force_override_detail: Option<String>,
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
        let ego_provider = providers.ego_kind.map(EgoProvider::from);

        // Auto-upgrade to CliOrchestrator when the ego is a CLI variant.
        // Tier-based scoring and model override are meaningless for CLI
        // providers since they manage their own model selection.
        let mode = match (&ego_provider, providers.routing_mode) {
            (
                Some(
                    EgoProvider::ClaudeCli
                    | EgoProvider::GeminiCli
                    | EgoProvider::CodexCli
                    | EgoProvider::GrokCli,
                ),
                RoutingMode::TierBased | RoutingMode::EgoPrimary,
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
            council: None,
            local_http: providers.local_http,
            mode,
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
            council_provider_count: self.council.as_ref().map_or(0, |c| c.provider_count()),
        }
    }

    /// Diagnose what the router would do for a given message without calling any LLM.
    pub fn diagnose(&self, user_message: &str) -> RoutingDiagnosis {
        let target = self.target_for_mode(user_message);
        let target_str = match target {
            FastPathTarget::Ego => "ego",
            FastPathTarget::Id => "id",
        };

        let (selected_tier, selection_reason, complexity_score) = match self.mode {
            RoutingMode::TierBased => {
                if let Some(ref model) = self.force_override.pinned_model {
                    (
                        None,
                        format!("pinned_model({})", model),
                        Some(self.calculate_id_instinct(user_message)),
                    )
                } else {
                    let (tier, reason, score) = self.select_tier_with_reason(user_message);
                    let tier_name = match tier {
                        ModelTier::Fast => "fast",
                        ModelTier::Standard => "standard",
                        ModelTier::Pro => "pro",
                    };
                    (Some(tier_name.to_string()), reason.to_string(), Some(score))
                }
            }
            RoutingMode::EgoPrimary => (None, "ego_primary".to_string(), None),
            RoutingMode::Council => (None, "council".to_string(), None),
            RoutingMode::CliOrchestrator => (None, "cli_orchestrator".to_string(), None),
        };

        let selected_model =
            if self.mode == RoutingMode::TierBased || self.mode == RoutingMode::Council {
                self.resolve_model_for_request(user_message)
            } else {
                None
            };

        let force_active =
            self.force_override.pinned_model.is_some() || self.force_override.pinned_tier.is_some();
        let force_detail = if force_active {
            Some(format!(
                "model={:?} tier={:?} provider={:?}",
                self.force_override.pinned_model,
                self.force_override.pinned_tier,
                self.force_override.pinned_provider,
            ))
        } else {
            None
        };

        RoutingDiagnosis {
            mode: format!("{:?}", self.mode).to_lowercase(),
            target: target_str.to_string(),
            selected_tier,
            selected_model,
            complexity_score,
            selection_reason,
            ego_provider: self.ego_provider.as_ref().map(|p| p.to_string()),
            has_local_llm: self.local_http.is_some(),
            council_available: self
                .council
                .as_ref()
                .is_some_and(|c| c.provider_count() >= 2),
            council_provider_count: self.council.as_ref().map_or(0, |c| c.provider_count()),
            force_override_active: force_active,
            force_override_detail: force_detail,
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
        self.select_tier_with_reason(user_message).0
    }

    /// Like `select_tier`, but also returns the reason and complexity score.
    pub fn select_tier_with_reason(&self, user_message: &str) -> (ModelTier, SelectionReason, u8) {
        let score = self.calculate_id_instinct(user_message);

        if let Some(tier) = self.force_override.pinned_tier {
            tracing::debug!("Tier pinned by force override: {:?}", tier);
            return (tier, SelectionReason::PinnedTier, score);
        }
        if Self::detect_setup_intent(user_message) {
            tracing::debug!("Setup/credential intent detected — escalating to Pro tier");
            return (ModelTier::Pro, SelectionReason::SetupIntent, score);
        }
        let tier = self.tier_thresholds.score_to_tier(score);
        tracing::debug!("Tier selected by complexity score {}: {:?}", score, tier);
        (tier, SelectionReason::Complexity, score)
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
        let no_real_local = self.local_http.is_none();
        let ego_feasible =
            self.ego.is_some() && (no_real_local || id_instinct >= 45 || context_aligned);

        let target = if ego_feasible
            && (no_real_local || id_instinct >= 60 || (context_aligned && id_instinct >= 20))
        {
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
        let mut trace = ExecutionTrace::new(
            &self.routing_mode_str(),
            self.ego_provider.as_ref().map(|p| p.to_string()),
            model_override.clone(),
            target_str,
        );

        match self.mode {
            RoutingMode::TierBased => {
                if self.force_override.pinned_model.is_some() {
                    trace.selection_reason = Some(SelectionReason::PinnedModel);
                    trace.configured_tier = None;
                    trace.complexity_score = Some(self.calculate_id_instinct(user_message));
                } else {
                    let (tier, reason, score) = self.select_tier_with_reason(user_message);
                    let tier_name = match tier {
                        ModelTier::Fast => "fast",
                        ModelTier::Standard => "standard",
                        ModelTier::Pro => "pro",
                    };
                    trace.configured_tier = Some(tier_name.to_string());
                    trace.complexity_score = Some(score);
                    trace.selection_reason = Some(reason);
                }
            }
            RoutingMode::EgoPrimary | RoutingMode::CliOrchestrator => {
                trace.selection_reason = Some(SelectionReason::EgoPrimary);
            }
            RoutingMode::Council => {
                trace.selection_reason = Some(SelectionReason::Council);
            }
        }

        trace
    }

    /// Traced variant of `route_fast` / `route`.
    pub async fn route_traced(
        &self,
        messages: Vec<Message>,
    ) -> anyhow::Result<(CompletionResponse, ExecutionTrace)> {
        let last_msg = messages.last().map_or("", |m| &m.content).to_string();

        // Council mode: delegate to the council engine when available with 2+ providers
        if self.mode == RoutingMode::Council {
            if let Some(ref council) = self.council {
                if council.provider_count() >= 2 {
                    return self
                        .route_council_traced(messages, &last_msg, council)
                        .await;
                }
                tracing::warn!(
                    "Council has {} provider(s) — degraded passthrough to single-provider path",
                    council.provider_count()
                );
            } else {
                tracing::warn!("Council mode active but no CouncilEngine configured — falling back to tier-based");
            }
        }

        let target = self.target_for_mode(&last_msg);
        let model_override =
            if self.mode == RoutingMode::TierBased || self.mode == RoutingMode::Council {
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

    /// Council-specific traced execution path.
    async fn route_council_traced(
        &self,
        messages: Vec<Message>,
        last_msg: &str,
        council: &CouncilEngine,
    ) -> anyhow::Result<(CompletionResponse, ExecutionTrace)> {
        let mut trace = self.begin_trace(last_msg, &None);
        trace.selection_reason = Some(SelectionReason::Council);

        let t0 = chrono::Utc::now();
        match council.deliberate(messages, None).await {
            Ok(result) => {
                let synthesis_provider = result
                    .drafts
                    .first()
                    .map(|d| d.provider.as_str())
                    .unwrap_or("council");
                let provider_label = format!(
                    "council({} providers, synthesis={})",
                    result.provider_count, synthesis_provider
                );
                trace.record_success(&provider_label, None, t0);

                let response = CompletionResponse {
                    content: result.synthesis,
                    tool_calls: None,
                };
                Ok((response, trace))
            }
            Err(e) => {
                trace.record_error("council", None, &e.to_string(), t0);
                tracing::warn!("Council deliberation failed, falling back to ego/id: {}", e);

                if let Some(ref ego) = self.ego {
                    let t1 = chrono::Utc::now();
                    let request = CompletionRequest::simple(vec![Message::new("user", last_msg)]);
                    let resp = ego.complete(&request).await?;
                    trace.record_success(&self.ego_label(), None, t1);
                    trace.selection_reason = Some(SelectionReason::Fallback);
                    return Ok((resp, trace));
                }

                Err(e)
            }
        }
    }

    /// Traced variant of `route_with_tools`.
    ///
    /// Council mode does not support tool-use (deliberation is text-only),
    /// so this falls through to the standard ego/id path with tier selection.
    pub async fn route_with_tools_traced(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> anyhow::Result<(CompletionResponse, ExecutionTrace)> {
        let last_msg = messages.last().map_or("", |m| &m.content).to_string();
        let target = self.target_for_mode(&last_msg);
        let model_override =
            if self.mode == RoutingMode::TierBased || self.mode == RoutingMode::Council {
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
    async fn test_fast_path_classify_routes_to_ego_when_no_local_llm() {
        // When no real local LLM is configured, even short messages must
        // go to Ego — the CandleProvider stub cannot serve real responses.
        let router = IdEgoRouter::new(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            None,
            RoutingMode::TierBased,
        );
        let fp = router.fast_path_classify("hi");
        assert_eq!(fp.target, FastPathTarget::Ego);
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

    // ── select_tier_with_reason tests ─────────────────────────────

    #[test]
    fn test_select_tier_with_reason_complexity() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        let (tier, reason, score) = router.select_tier_with_reason("hi");
        assert_eq!(tier, ModelTier::Fast);
        assert_eq!(reason, SelectionReason::Complexity);
        assert!(score < 35);
    }

    #[test]
    fn test_select_tier_with_reason_pinned_tier() {
        let mut router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        router.force_override.pinned_tier = Some(ModelTier::Pro);
        let (tier, reason, _) = router.select_tier_with_reason("hi");
        assert_eq!(tier, ModelTier::Pro);
        assert_eq!(reason, SelectionReason::PinnedTier);
    }

    #[test]
    fn test_select_tier_with_reason_setup_intent() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        let (tier, reason, _) = router.select_tier_with_reason("configure my IMAP credentials");
        assert_eq!(tier, ModelTier::Pro);
        assert_eq!(reason, SelectionReason::SetupIntent);
    }

    // ── begin_trace tier field tests ──────────────────────────────

    #[test]
    fn test_begin_trace_populates_tier_fields() {
        let router = IdEgoRouter::new(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            None,
            RoutingMode::TierBased,
        );
        let trace = router.begin_trace("hi", &None);
        assert!(trace.configured_tier.is_some());
        assert_eq!(trace.configured_tier.as_deref(), Some("fast"));
        assert!(trace.complexity_score.is_some());
        assert_eq!(trace.selection_reason, Some(SelectionReason::Complexity));
    }

    #[test]
    fn test_begin_trace_pinned_model_reason() {
        let mut router = IdEgoRouter::new(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            None,
            RoutingMode::TierBased,
        );
        router.force_override.pinned_model = Some("custom-v9".to_string());
        let trace = router.begin_trace("hi", &Some("custom-v9".to_string()));
        assert_eq!(trace.selection_reason, Some(SelectionReason::PinnedModel));
        assert!(trace.configured_tier.is_none());
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

    #[test]
    fn test_begin_trace_council_reason() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::Council);
        let trace = router.begin_trace("hi", &None);
        assert_eq!(trace.selection_reason, Some(SelectionReason::Council));
    }

    // ── diagnose tests ────────────────────────────────────────────

    #[test]
    fn test_diagnose_tier_based_simple() {
        let router = IdEgoRouter::new(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            None,
            RoutingMode::TierBased,
        );
        let diag = router.diagnose("hi");
        assert_eq!(diag.mode, "tierbased");
        assert_eq!(diag.selected_tier.as_deref(), Some("fast"));
        assert!(diag.complexity_score.is_some());
        assert_eq!(diag.selection_reason, "complexity");
        assert!(!diag.force_override_active);
    }

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
        assert!(diag.selected_tier.is_none());
    }

    #[test]
    fn test_diagnose_force_override_active() {
        let mut router = IdEgoRouter::new(
            None,
            Some("openai"),
            Some("test-key".to_string()),
            None,
            RoutingMode::TierBased,
        );
        router.force_override.pinned_model = Some("custom-v9".to_string());
        let diag = router.diagnose("hi");
        assert!(diag.force_override_active);
        assert!(diag.force_override_detail.is_some());
        assert_eq!(diag.selected_model, Some("custom-v9".to_string()));
    }

    #[test]
    fn test_diagnose_council_mode() {
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::Council);
        let diag = router.diagnose("hi");
        assert_eq!(diag.mode, "council");
        assert_eq!(diag.selection_reason, "council");
        assert!(!diag.council_available);
        assert_eq!(diag.council_provider_count, 0);
    }

    // ── Trace final_tier tests ────────────────────────────────────

    #[test]
    fn test_trace_final_tier_no_fallback() {
        let mut trace = ExecutionTrace::new("tierbased", Some("openai".into()), None, "ego");
        trace.configured_tier = Some("fast".to_string());
        let t0 = chrono::Utc::now();
        trace.record_success("openai", Some("gpt-4.1-mini".into()), t0);
        assert_eq!(trace.final_tier(), Some("fast"));
    }

    #[test]
    fn test_trace_final_tier_fallback_clears_tier() {
        let mut trace = ExecutionTrace::new("tierbased", Some("openai".into()), None, "ego");
        trace.configured_tier = Some("pro".to_string());
        let t0 = chrono::Utc::now();
        trace.record_error("openai", None, "timeout", t0);
        let t1 = chrono::Utc::now();
        trace.record_success("id(candle_stub)", None, t1);
        assert!(trace.fallback_occurred);
        assert_eq!(trace.final_tier(), None);
    }
}
