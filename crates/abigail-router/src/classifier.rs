//! Prompt complexity classifier for multi-tier routing.
//!
//! Two-layer classification:
//! - **Layer 1**: Deterministic 5-factor decision matrix (fast, no LLM call)
//! - **Layer 2**: LLM-based fallback when Layer 1 confidence is low
//!
//! ## Decision Matrix Factors
//!
//! Each factor is scored 0–100 independently, then combined with configurable
//! weights to produce a composite score (0–100). The composite score determines
//! both the `PromptTier` and the `RoutingTarget`.
//!
//! | Factor | Weight | Description |
//! |--------|--------|-------------|
//! | Complexity | 0.30 | Structural complexity: word count, nesting, multi-step markers |
//! | Ethical weight | 0.15 | Sensitivity: safety, PII, medical/legal/financial topics |
//! | Latency tolerance | 0.15 | How patient the user likely is (inverted urgency) |
//! | Tool count | 0.20 | Number of tool-use signals detected |
//! | User preference | 0.20 | Explicit routing hints or session-level preference |

use abigail_capabilities::cognitive::{CompletionRequest, LlmProvider, Message};
use std::sync::Arc;

// ── Routing target ───────────────────────────────────────────────────

/// Which subsystem should handle this request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingTarget {
    /// Local LLM (Id) — fast, private, lower capability.
    Id,
    /// Cloud LLM (Ego) — higher capability, higher latency/cost.
    Ego,
    /// Safety-critical path (Superego) — requires ethical review before routing.
    /// Future: automatically triggered when ethical_weight is very high.
    Superego,
}

impl std::fmt::Display for RoutingTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RoutingTarget::Id => write!(f, "Id"),
            RoutingTarget::Ego => write!(f, "Ego"),
            RoutingTarget::Superego => write!(f, "Superego"),
        }
    }
}

// ── Decision matrix ──────────────────────────────────────────────────

/// Five-factor decision matrix for routing decisions.
///
/// Each factor is scored 0–100. Higher = needs more powerful model / cloud routing.
#[derive(Debug, Clone)]
pub struct DecisionMatrix {
    /// Structural complexity of the request (word count, nesting, multi-step).
    pub complexity: u8,
    /// Ethical/safety sensitivity (PII, medical, legal, financial topics).
    pub ethical_weight: u8,
    /// Latency tolerance — higher means user can wait longer (analysis, research).
    /// Lower means user expects instant response (greetings, lookups).
    pub latency_tolerance: u8,
    /// Tool-use signal count — how many distinct tools the request likely needs.
    pub tool_count: u8,
    /// User preference — explicit routing hints or session-level override.
    /// 50 = neutral (no preference expressed).
    pub user_preference: u8,
}

impl DecisionMatrix {
    /// Compute the weighted composite score (0–100).
    pub fn composite(&self) -> u8 {
        self.composite_with_weights(&FactorWeights::default())
    }

    /// Compute composite score with custom weights.
    pub fn composite_with_weights(&self, w: &FactorWeights) -> u8 {
        let raw = (self.complexity as f32) * w.complexity
            + (self.ethical_weight as f32) * w.ethical_weight
            + (self.latency_tolerance as f32) * w.latency_tolerance
            + (self.tool_count as f32) * w.tool_count
            + (self.user_preference as f32) * w.user_preference;
        (raw.round() as u8).min(100)
    }

    /// Advisory tier based purely on composite score.
    /// In practice, `ClassificationResult::tier` is set by signal detection for precision.
    pub fn advisory_tier(&self) -> PromptTier {
        match self.composite() {
            0..=20 => PromptTier::T1Fast,
            21..=45 => PromptTier::T2Standard,
            46..=70 => PromptTier::T3Pro,
            71..=100 => PromptTier::T4Specialist,
            _ => PromptTier::T2Standard,
        }
    }

    /// Map composite score to a RoutingTarget.
    pub fn to_routing_target(&self) -> RoutingTarget {
        // Ethical weight ≥ 80 → Superego review (future)
        if self.ethical_weight >= 80 {
            return RoutingTarget::Superego;
        }
        score_to_routing_target(self.composite())
    }
}

impl std::fmt::Display for DecisionMatrix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "score={} [complexity={}, ethical={}, latency={}, tools={}, preference={}]",
            self.composite(),
            self.complexity,
            self.ethical_weight,
            self.latency_tolerance,
            self.tool_count,
            self.user_preference,
        )
    }
}

/// Configurable weights for each factor. Must sum to ~1.0.
#[derive(Debug, Clone)]
pub struct FactorWeights {
    pub complexity: f32,
    pub ethical_weight: f32,
    pub latency_tolerance: f32,
    pub tool_count: f32,
    pub user_preference: f32,
}

impl Default for FactorWeights {
    fn default() -> Self {
        Self {
            complexity: 0.30,
            ethical_weight: 0.15,
            latency_tolerance: 0.15,
            tool_count: 0.20,
            user_preference: 0.20,
        }
    }
}

// ── Threshold functions ──────────────────────────────────────────────

/// Composite score → RoutingTarget.
///
/// The composite score determines WHERE to route (Id vs Ego), not which tier.
/// Tier is determined by signal detection for precision; the composite provides
/// the overall "how demanding is this request" metric for Id/Ego selection.
fn score_to_routing_target(score: u8) -> RoutingTarget {
    match score {
        0..=30 => RoutingTarget::Id,
        31..=100 => RoutingTarget::Ego,
        _ => RoutingTarget::Ego,
    }
}

/// Determine PromptTier from detected signals (strongest signal wins).
///
/// This preserves the original classification logic: specific patterns map
/// to specific tiers with high precision. The composite score supplements
/// this with a holistic routing target, but tier selection is signal-driven.
fn tier_from_signals(signals: &Signals) -> PromptTier {
    // T4Specialist: code and math
    if signals.is_code_request || signals.is_code_content || signals.is_math_request {
        return PromptTier::T4Specialist;
    }
    // T3Pro: complex analysis, creative, tool use, long messages
    if signals.is_complex_analysis
        || signals.is_creative_writing
        || signals.is_tool_use
        || signals.is_long_message
    {
        return PromptTier::T3Pro;
    }
    // T1Fast: greetings, simple questions, short messages
    if signals.is_greeting || signals.is_simple_question || signals.is_short_message {
        return PromptTier::T1Fast;
    }
    // Default: T2Standard
    PromptTier::T2Standard
}

// ── Prompt tier ──────────────────────────────────────────────────────

/// Prompt complexity tier for routing decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptTier {
    /// Quick factual lookups, greetings, simple commands (local or fast cloud model)
    T1Fast,
    /// Standard conversation, summaries, moderate reasoning (standard model)
    T2Standard,
    /// Complex analysis, multi-step reasoning, creative writing (pro model)
    T3Pro,
    /// Domain-specific: code generation, math proofs, research (specialized/pro model)
    T4Specialist,
}

impl std::fmt::Display for PromptTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PromptTier::T1Fast => write!(f, "T1Fast"),
            PromptTier::T2Standard => write!(f, "T2Standard"),
            PromptTier::T3Pro => write!(f, "T3Pro"),
            PromptTier::T4Specialist => write!(f, "T4Specialist"),
        }
    }
}

/// Maps PromptTier to the existing ModelTier used in config.
impl From<PromptTier> for abigail_core::ModelTier {
    fn from(tier: PromptTier) -> Self {
        match tier {
            PromptTier::T1Fast => abigail_core::ModelTier::Fast,
            PromptTier::T2Standard => abigail_core::ModelTier::Standard,
            PromptTier::T3Pro | PromptTier::T4Specialist => abigail_core::ModelTier::Pro,
        }
    }
}

// ── Classification result ────────────────────────────────────────────

/// Result of classifying a prompt.
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub tier: PromptTier,
    /// Confidence score 0.0–1.0
    pub confidence: f32,
    /// Which rule or method produced this classification (for diagnostics)
    pub matched_rule: Option<String>,
    /// Five-factor decision matrix breakdown.
    pub matrix: DecisionMatrix,
    /// Recommended routing target derived from the matrix.
    pub routing_target: RoutingTarget,
}

/// Minimum Layer 1 confidence before falling back to Layer 2 (LLM).
const L2_CONFIDENCE_THRESHOLD: f32 = 0.5;

/// Timeout for Layer 2 LLM classification (milliseconds).
const L2_TIMEOUT_MS: u64 = 3000;

/// Default user preference when no explicit hint is given.
const DEFAULT_USER_PREFERENCE: u8 = 50;

/// Classifies prompts into complexity tiers using a 5-factor decision matrix.
pub struct PromptClassifier {
    /// Optional LLM for Layer 2 fallback classification.
    llm: Option<Arc<dyn LlmProvider>>,
    /// Factor weights (configurable per-instance).
    weights: FactorWeights,
}

impl PromptClassifier {
    /// Create a classifier with an optional LLM for Layer 2 fallback.
    pub fn new(llm: Option<Arc<dyn LlmProvider>>) -> Self {
        Self {
            llm,
            weights: FactorWeights::default(),
        }
    }

    /// Create a classifier with custom factor weights.
    pub fn with_weights(llm: Option<Arc<dyn LlmProvider>>, weights: FactorWeights) -> Self {
        Self { llm, weights }
    }

    /// Classify a user message into a PromptTier using the 5-factor decision matrix.
    pub async fn classify(&self, message: &str) -> ClassificationResult {
        let l1_result = self.classify_layer1(message);

        tracing::debug!(
            "L1 classification: {} → {} (target={})",
            l1_result.matrix,
            l1_result.tier,
            l1_result.routing_target,
        );

        // If Layer 1 is confident enough, use it directly
        if l1_result.confidence >= L2_CONFIDENCE_THRESHOLD {
            return l1_result;
        }

        // Try Layer 2 LLM fallback
        if let Some(ref llm) = self.llm {
            match self.classify_layer2(llm.as_ref(), message).await {
                Some(l2_result) => l2_result,
                None => l1_result, // Timeout or failure → use Layer 1
            }
        } else {
            l1_result
        }
    }

    /// Layer 1: Five-factor decision matrix classification (fast, no LLM).
    ///
    /// Computes each factor score independently, then combines them with weights
    /// to produce a composite score that maps to a PromptTier and RoutingTarget.
    fn classify_layer1(&self, message: &str) -> ClassificationResult {
        let lower = message.trim().to_lowercase();
        let word_count = message.split_whitespace().count();

        // Detect which patterns are present (used by multiple factors)
        let signals = detect_signals(&lower, message, word_count);

        // ── Factor 1: Complexity (0–100) ──
        let complexity = score_complexity(&lower, word_count, &signals);

        // ── Factor 2: Ethical weight (0–100) ──
        let ethical_weight = score_ethical_weight(&lower);

        // ── Factor 3: Latency tolerance (0–100) ──
        let latency_tolerance = score_latency_tolerance(&signals);

        // ── Factor 4: Tool count (0–100) ──
        let tool_count = score_tool_count(&lower);

        // ── Factor 5: User preference (0–100) ──
        let user_preference = score_user_preference(&lower);

        let matrix = DecisionMatrix {
            complexity,
            ethical_weight,
            latency_tolerance,
            tool_count,
            user_preference,
        };

        let tier = tier_from_signals(&signals);
        let routing_target = matrix.to_routing_target();
        let composite = matrix.composite_with_weights(&self.weights);

        // Confidence: how decisive the classification is.
        // Strong signals → high confidence. Mid-range composite → lower confidence → may trigger L2.
        let confidence = compute_confidence(composite, &signals);

        // Pick the most descriptive matched rule from the signals
        let matched_rule = signals.primary_rule();

        ClassificationResult {
            tier,
            confidence,
            matched_rule,
            matrix,
            routing_target,
        }
    }

    /// Layer 2: LLM-based classification fallback.
    /// Returns None on timeout or failure.
    async fn classify_layer2(
        &self,
        llm: &dyn LlmProvider,
        message: &str,
    ) -> Option<ClassificationResult> {
        let prompt = format!(
            "Classify this user message into exactly one complexity tier. Reply with ONLY a JSON object.\n\n\
             Tiers:\n\
             - T1: Simple greetings, factual lookups, yes/no questions, time/date\n\
             - T2: Standard conversation, summaries, definitions, moderate questions\n\
             - T3: Complex analysis, essays, multi-step reasoning, creative writing\n\
             - T4: Code generation, math proofs, specialized domain tasks\n\n\
             User message: \"{}\"\n\n\
             Reply ONLY with: {{\"tier\": \"T1\"|\"T2\"|\"T3\"|\"T4\", \"confidence\": 0.0-1.0}}",
            message
        );

        let request = CompletionRequest::simple(vec![Message::new("user", prompt)]);

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(L2_TIMEOUT_MS),
            llm.complete(&request),
        )
        .await;

        match result {
            Ok(Ok(response)) => parse_l2_response(&response.content),
            Ok(Err(e)) => {
                tracing::warn!("Layer 2 classification LLM error: {}", e);
                None
            }
            Err(_) => {
                tracing::warn!("Layer 2 classification timed out ({}ms)", L2_TIMEOUT_MS);
                None
            }
        }
    }
}

// ── Signal detection ─────────────────────────────────────────────────

/// Detected signal categories from the message. Used by multiple factor scorers.
#[derive(Debug, Default)]
struct Signals {
    is_greeting: bool,
    is_short_message: bool,
    is_simple_question: bool,
    is_code_request: bool,
    is_code_content: bool,
    is_math_request: bool,
    is_complex_analysis: bool,
    is_creative_writing: bool,
    is_tool_use: bool,
    is_long_message: bool,
    tool_signal_count: u8,
}

impl Signals {
    /// Return the most descriptive rule name for diagnostics.
    fn primary_rule(&self) -> Option<String> {
        if self.is_greeting {
            Some("greeting".to_string())
        } else if self.is_simple_question {
            Some("simple_question".to_string())
        } else if self.is_short_message {
            Some("short_message".to_string())
        } else if self.is_code_request {
            Some("code_request".to_string())
        } else if self.is_code_content {
            Some("code_content".to_string())
        } else if self.is_math_request {
            Some("math_request".to_string())
        } else if self.is_complex_analysis {
            Some("complex_analysis".to_string())
        } else if self.is_creative_writing {
            Some("creative_writing".to_string())
        } else if self.is_tool_use {
            Some("tool_use_request".to_string())
        } else if self.is_long_message {
            Some("long_message".to_string())
        } else {
            Some("default".to_string())
        }
    }
}

fn detect_signals(lower: &str, original: &str, word_count: usize) -> Signals {
    let mut s = Signals::default();

    // Greetings
    let greetings = [
        "hi",
        "hello",
        "hey",
        "yo",
        "sup",
        "howdy",
        "greetings",
        "good morning",
        "good afternoon",
        "good evening",
        "good night",
        "thanks",
        "thank you",
        "bye",
        "goodbye",
        "ok",
        "okay",
        "yes",
        "no",
        "sure",
        "yep",
        "nope",
    ];
    for g in &greetings {
        if lower == *g || lower == format!("{}!", g) || lower == format!("{}.", g) {
            s.is_greeting = true;
            break;
        }
    }

    // Short messages (≤3 words, not a question or command)
    if word_count <= 3
        && !lower.contains('?')
        && !lower.starts_with("write")
        && !lower.starts_with("explain")
        && !lower.starts_with("analyze")
    {
        s.is_short_message = true;
    }

    // Simple time/date questions
    let simple_q = [
        "what time",
        "what's the time",
        "what day",
        "what's the date",
        "what is the date",
        "what is the time",
        "current time",
        "current date",
        "what is today",
        "what's today",
    ];
    for pat in &simple_q {
        if lower.contains(pat) {
            s.is_simple_question = true;
            break;
        }
    }

    // Code request markers
    let code_markers = [
        "write a function",
        "write a program",
        "write code",
        "implement a",
        "debug this",
        "fix this code",
        "refactor",
        "code review",
        "write a script",
        "write a class",
        "write a method",
        "write a test",
        "write a module",
        "write an api",
        "write a query",
        "sql query",
        "regex for",
        "regular expression",
    ];
    for pat in &code_markers {
        if lower.contains(pat) {
            s.is_code_request = true;
            break;
        }
    }

    // Code content (fences, syntax)
    if original.contains("```")
        || original.contains("fn ")
        || original.contains("def ")
        || original.contains("class ")
        || original.contains("function ")
        || original.contains("import ")
        || original.contains("#include")
    {
        s.is_code_content = true;
    }

    // Math markers
    let math_markers = [
        "solve",
        "prove",
        "calculate",
        "compute",
        "derive",
        "integrate",
        "differentiate",
        "equation",
        "theorem",
        "mathematical",
        "probability",
        "statistics",
        "matrix",
        "eigenvalue",
    ];
    for pat in &math_markers {
        if lower.contains(pat) {
            s.is_math_request = true;
            break;
        }
    }

    // Complex analysis markers
    let complex_markers = [
        "analyze",
        "analyse",
        "compare and contrast",
        "write an essay",
        "explain in detail",
        "in-depth",
        "comprehensive",
        "thorough analysis",
        "critical analysis",
        "evaluate the",
        "pros and cons",
        "implications of",
        "discuss the impact",
        "multi-step",
        "step by step",
        "detailed explanation",
        "research paper",
        "literature review",
    ];
    for pat in &complex_markers {
        if lower.contains(pat) {
            s.is_complex_analysis = true;
            break;
        }
    }

    // Creative writing markers
    let creative_markers = [
        "write a story",
        "write a poem",
        "creative writing",
        "write a novel",
        "write a song",
        "write lyrics",
        "fictional",
        "short story",
        "write a speech",
        "write a letter",
    ];
    for pat in &creative_markers {
        if lower.contains(pat) {
            s.is_creative_writing = true;
            break;
        }
    }

    // Tool-use patterns (count distinct matches)
    let tool_patterns = [
        "search the web",
        "search for",
        "look up",
        "find online",
        "read file",
        "write file",
        "read the file",
        "write to file",
        "make a request",
        "fetch url",
        "http get",
        "browse to",
        "download",
        "send email",
        "check email",
    ];
    let mut tool_hits: u8 = 0;
    for pat in &tool_patterns {
        if lower.contains(pat) {
            tool_hits += 1;
        }
    }
    if tool_hits > 0 {
        s.is_tool_use = true;
        s.tool_signal_count = tool_hits;
    }

    // Long message
    if word_count > 100 {
        s.is_long_message = true;
    }

    s
}

// ── Factor scoring functions ─────────────────────────────────────────

/// Factor 1: Complexity — structural complexity of the request.
fn score_complexity(lower: &str, word_count: usize, signals: &Signals) -> u8 {
    let mut score: u16 = 0;

    // Word count contribution
    score += match word_count {
        0..=3 => 5,
        4..=10 => 15,
        11..=30 => 30,
        31..=60 => 45,
        61..=100 => 60,
        _ => 75,
    };

    // Question marks add complexity (multi-question prompts)
    let q_count = lower.chars().filter(|c| *c == '?').count();
    score += (q_count as u16 * 5).min(15);

    // Multi-step markers
    if lower.contains("step by step")
        || lower.contains("first")
            && (lower.contains("then") || lower.contains("next") || lower.contains("finally"))
    {
        score += 15;
    }

    // Code signals are highly complex
    if signals.is_code_request || signals.is_code_content {
        score += 25;
    }

    // Math signals
    if signals.is_math_request {
        score += 25;
    }

    // Complex analysis / creative writing
    if signals.is_complex_analysis {
        score += 20;
    }
    if signals.is_creative_writing {
        score += 15;
    }

    // Simple signals reduce complexity
    if signals.is_greeting {
        return 5;
    }
    if signals.is_simple_question {
        return 10;
    }

    (score as u8).min(100)
}

/// Factor 2: Ethical weight — sensitivity of the topic.
fn score_ethical_weight(lower: &str) -> u8 {
    let mut score: u16 = 5; // baseline: most messages are ethically neutral

    let sensitive_patterns = [
        ("medical advice", 30),
        ("legal advice", 30),
        ("financial advice", 25),
        ("investment", 20),
        ("diagnosis", 30),
        ("prescription", 30),
        ("therapy", 20),
        ("suicide", 50),
        ("self-harm", 50),
        ("violence", 40),
        ("weapon", 40),
        ("hack", 30),
        ("exploit", 25),
        ("personal data", 25),
        ("password", 20),
        ("social security", 35),
        ("credit card", 25),
        ("children", 15),
        ("minor", 15),
    ];

    for (pat, weight) in &sensitive_patterns {
        if lower.contains(pat) {
            score += *weight as u16;
        }
    }

    (score as u8).min(100)
}

/// Factor 3: Latency tolerance — higher = user can wait (complex task).
fn score_latency_tolerance(signals: &Signals) -> u8 {
    if signals.is_greeting {
        return 5;
    }
    if signals.is_simple_question || signals.is_short_message {
        return 10;
    }
    if signals.is_code_request || signals.is_code_content || signals.is_math_request {
        return 85;
    }
    if signals.is_complex_analysis {
        return 75;
    }
    if signals.is_creative_writing {
        return 70;
    }
    if signals.is_tool_use {
        return 65;
    }
    if signals.is_long_message {
        return 60;
    }
    // Default: moderate tolerance
    40
}

/// Factor 4: Tool count — how many tools the request likely needs.
fn score_tool_count(lower: &str) -> u8 {
    let tool_patterns = [
        "search the web",
        "search for",
        "look up",
        "find online",
        "read file",
        "write file",
        "read the file",
        "write to file",
        "make a request",
        "fetch url",
        "http get",
        "browse to",
        "download",
        "send email",
        "check email",
        "run command",
        "execute",
        "open",
        "save",
        "upload",
    ];

    let mut hits: u8 = 0;
    for pat in &tool_patterns {
        if lower.contains(pat) {
            hits += 1;
        }
    }

    match hits {
        0 => 0,
        1 => 30,
        2 => 60,
        _ => 90,
    }
}

/// Factor 5: User preference — explicit routing hints.
fn score_user_preference(lower: &str) -> u8 {
    // Explicit "use local" / "quick" / "fast" → prefer Id
    let local_hints = [
        "use local",
        "quick answer",
        "fast",
        "briefly",
        "tldr",
        "tl;dr",
    ];
    for pat in &local_hints {
        if lower.contains(pat) {
            return 15;
        }
    }

    // Explicit "best quality" / "thorough" / "use cloud" → prefer Ego
    let cloud_hints = [
        "best quality",
        "thorough",
        "use cloud",
        "detailed",
        "in depth",
        "comprehensive",
    ];
    for pat in &cloud_hints {
        if lower.contains(pat) {
            return 85;
        }
    }

    DEFAULT_USER_PREFERENCE
}

/// Compute confidence from detected signals.
///
/// Strong signals → high confidence (L2 skipped).
/// No strong signal → low confidence (0.40) → triggers L2 if available.
fn compute_confidence(_composite: u8, signals: &Signals) -> f32 {
    if signals.is_greeting {
        return 0.95;
    }
    if signals.is_simple_question {
        return 0.90;
    }
    if signals.is_code_request || signals.is_code_content {
        return 0.85;
    }
    if signals.is_math_request {
        return 0.80;
    }
    if signals.is_complex_analysis {
        return 0.80;
    }
    if signals.is_creative_writing {
        return 0.75;
    }
    if signals.is_tool_use {
        return 0.75;
    }
    if signals.is_short_message {
        return 0.60;
    }
    if signals.is_long_message {
        return 0.60;
    }

    // No strong signal detected → low confidence, may trigger L2 fallback
    0.40
}

/// Parse the Layer 2 LLM response JSON.
fn parse_l2_response(content: &str) -> Option<ClassificationResult> {
    // Try to find JSON in the response
    let json_str = content
        .find('{')
        .and_then(|start| content.rfind('}').map(|end| &content[start..=end]))?;

    let parsed: serde_json::Value = serde_json::from_str(json_str).ok()?;

    let tier_str = parsed.get("tier")?.as_str()?;
    let confidence = parsed
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.7) as f32;

    let tier = match tier_str {
        "T1" => PromptTier::T1Fast,
        "T2" => PromptTier::T2Standard,
        "T3" => PromptTier::T3Pro,
        "T4" => PromptTier::T4Specialist,
        _ => return None,
    };

    // For L2 results, build a neutral matrix (L2 doesn't provide factor breakdown)
    let composite = match tier {
        PromptTier::T1Fast => 10,
        PromptTier::T2Standard => 35,
        PromptTier::T3Pro => 60,
        PromptTier::T4Specialist => 85,
    };
    let matrix = DecisionMatrix {
        complexity: composite,
        ethical_weight: 5,
        latency_tolerance: composite,
        tool_count: 0,
        user_preference: DEFAULT_USER_PREFERENCE,
    };
    let routing_target = matrix.to_routing_target();

    Some(ClassificationResult {
        tier,
        confidence,
        matched_rule: Some("llm_layer2".to_string()),
        matrix,
        routing_target,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classifier() -> PromptClassifier {
        PromptClassifier::new(None)
    }

    #[tokio::test]
    async fn test_greeting_t1() {
        let c = classifier();
        let r = c.classify("hello").await;
        assert_eq!(r.tier, PromptTier::T1Fast);
        assert!(r.confidence >= 0.9);
        assert_eq!(r.matched_rule.as_deref(), Some("greeting"));
    }

    #[tokio::test]
    async fn test_greeting_variants() {
        let c = classifier();
        for greeting in &["hi", "hey", "bye", "thanks", "ok", "yes", "no"] {
            let r = c.classify(greeting).await;
            assert_eq!(
                r.tier,
                PromptTier::T1Fast,
                "Expected T1Fast for '{}'",
                greeting
            );
        }
    }

    #[tokio::test]
    async fn test_time_question_t1() {
        let c = classifier();
        let r = c.classify("What time is it?").await;
        assert_eq!(r.tier, PromptTier::T1Fast);
        assert_eq!(r.matched_rule.as_deref(), Some("simple_question"));
    }

    #[tokio::test]
    async fn test_code_request_t4() {
        let c = classifier();
        let r = c.classify("Write a function to sort an array").await;
        assert_eq!(r.tier, PromptTier::T4Specialist);
        assert_eq!(r.matched_rule.as_deref(), Some("code_request"));
    }

    #[tokio::test]
    async fn test_code_content_t4() {
        let c = classifier();
        let r = c
            .classify("Can you fix this?\n```rust\nfn main() {}\n```")
            .await;
        assert_eq!(r.tier, PromptTier::T4Specialist);
        assert_eq!(r.matched_rule.as_deref(), Some("code_content"));
    }

    #[tokio::test]
    async fn test_math_request_t4() {
        let c = classifier();
        let r = c.classify("Solve this equation: 3x + 5 = 20").await;
        assert_eq!(r.tier, PromptTier::T4Specialist);
        assert_eq!(r.matched_rule.as_deref(), Some("math_request"));
    }

    #[tokio::test]
    async fn test_complex_analysis_t3() {
        let c = classifier();
        let r = c.classify("Analyze the pros and cons of remote work").await;
        assert_eq!(r.tier, PromptTier::T3Pro);
        assert_eq!(r.matched_rule.as_deref(), Some("complex_analysis"));
    }

    #[tokio::test]
    async fn test_essay_request_t3() {
        let c = classifier();
        let r = c
            .classify("Write an essay on the economic implications of AI")
            .await;
        assert_eq!(r.tier, PromptTier::T3Pro);
        assert_eq!(r.matched_rule.as_deref(), Some("complex_analysis"));
    }

    #[tokio::test]
    async fn test_creative_writing_t3() {
        let c = classifier();
        let r = c.classify("Write a short story about a dragon").await;
        assert_eq!(r.tier, PromptTier::T3Pro);
        assert_eq!(r.matched_rule.as_deref(), Some("creative_writing"));
    }

    #[tokio::test]
    async fn test_standard_question_t2() {
        let c = classifier();
        let r = c.classify("Tell me about dogs").await;
        assert_eq!(r.tier, PromptTier::T2Standard);
    }

    #[tokio::test]
    async fn test_moderate_question_t2() {
        let c = classifier();
        let r = c
            .classify("What is photosynthesis and how does it work?")
            .await;
        assert_eq!(r.tier, PromptTier::T2Standard);
    }

    #[tokio::test]
    async fn test_short_message_t1() {
        let c = classifier();
        let r = c.classify("cool").await;
        assert_eq!(r.tier, PromptTier::T1Fast);
    }

    #[tokio::test]
    async fn test_l2_response_parsing() {
        let result = parse_l2_response(r#"{"tier": "T3", "confidence": 0.85}"#);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.tier, PromptTier::T3Pro);
        assert!((r.confidence - 0.85).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_l2_response_parsing_with_extra_text() {
        let result = parse_l2_response(
            "Here is my classification: {\"tier\": \"T1\", \"confidence\": 0.9} done",
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().tier, PromptTier::T1Fast);
    }

    #[tokio::test]
    async fn test_l2_response_parsing_invalid() {
        assert!(parse_l2_response("not json").is_none());
        assert!(parse_l2_response(r#"{"tier": "T5"}"#).is_none());
    }

    #[tokio::test]
    async fn test_prompt_tier_to_model_tier() {
        use abigail_core::ModelTier;
        assert_eq!(ModelTier::from(PromptTier::T1Fast), ModelTier::Fast);
        assert_eq!(ModelTier::from(PromptTier::T2Standard), ModelTier::Standard);
        assert_eq!(ModelTier::from(PromptTier::T3Pro), ModelTier::Pro);
        assert_eq!(ModelTier::from(PromptTier::T4Specialist), ModelTier::Pro);
    }

    #[tokio::test]
    async fn test_default_confidence_triggers_l2_threshold() {
        let c = classifier();
        let r = c.classify("Tell me about dogs").await;
        // Default T2Standard should have low confidence < L2_CONFIDENCE_THRESHOLD
        // But without an LLM, it still returns T2Standard
        assert_eq!(r.tier, PromptTier::T2Standard);
        assert!(r.confidence < L2_CONFIDENCE_THRESHOLD);
    }

    // ── New coverage tests ─────────────────────────────────────────

    #[tokio::test]
    async fn test_empty_string_classifies() {
        let c = classifier();
        let r = c.classify("").await;
        assert_eq!(r.tier, PromptTier::T1Fast, "empty string → T1Fast");
    }

    #[tokio::test]
    async fn test_whitespace_only_classifies() {
        let c = classifier();
        let r = c.classify("  \n\t  ").await;
        assert_eq!(r.tier, PromptTier::T1Fast, "whitespace only → T1Fast");
    }

    #[tokio::test]
    async fn test_greeting_with_exclamation() {
        let c = classifier();
        let r = c.classify("Hello!").await;
        assert_eq!(r.tier, PromptTier::T1Fast);
        assert_eq!(r.matched_rule.as_deref(), Some("greeting"));
    }

    #[tokio::test]
    async fn test_greeting_with_period() {
        let c = classifier();
        let r = c.classify("bye.").await;
        assert_eq!(r.tier, PromptTier::T1Fast);
        assert_eq!(r.matched_rule.as_deref(), Some("greeting"));
    }

    #[tokio::test]
    async fn test_confidence_at_threshold() {
        let c = classifier();
        // Default-tier message → confidence < 0.5
        let r = c
            .classify("What is the meaning of life in general terms?")
            .await;
        assert!(
            r.confidence < L2_CONFIDENCE_THRESHOLD,
            "default tier confidence should be below L2 threshold"
        );
    }

    /// Mock LLM that returns a specific tier classification.
    struct MockClassifierLlm {
        response: String,
    }

    #[async_trait::async_trait]
    impl LlmProvider for MockClassifierLlm {
        async fn complete(
            &self,
            _: &CompletionRequest,
        ) -> anyhow::Result<abigail_capabilities::cognitive::CompletionResponse> {
            Ok(abigail_capabilities::cognitive::CompletionResponse {
                content: self.response.clone(),
                tool_calls: None,
            })
        }
        async fn stream(
            &self,
            req: &CompletionRequest,
            _tx: tokio::sync::mpsc::Sender<abigail_capabilities::cognitive::StreamEvent>,
        ) -> anyhow::Result<abigail_capabilities::cognitive::CompletionResponse> {
            self.complete(req).await
        }
    }

    /// Mock LLM that always fails.
    struct FailingClassifierLlm;

    #[async_trait::async_trait]
    impl LlmProvider for FailingClassifierLlm {
        async fn complete(
            &self,
            _: &CompletionRequest,
        ) -> anyhow::Result<abigail_capabilities::cognitive::CompletionResponse> {
            Err(anyhow::anyhow!("mock LLM error"))
        }
        async fn stream(
            &self,
            _: &CompletionRequest,
            _tx: tokio::sync::mpsc::Sender<abigail_capabilities::cognitive::StreamEvent>,
        ) -> anyhow::Result<abigail_capabilities::cognitive::CompletionResponse> {
            Err(anyhow::anyhow!("mock LLM stream error"))
        }
    }

    /// Mock LLM that sleeps longer than L2 timeout.
    struct SlowClassifierLlm;

    #[async_trait::async_trait]
    impl LlmProvider for SlowClassifierLlm {
        async fn complete(
            &self,
            _: &CompletionRequest,
        ) -> anyhow::Result<abigail_capabilities::cognitive::CompletionResponse> {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            Ok(abigail_capabilities::cognitive::CompletionResponse {
                content: r#"{"tier": "T4", "confidence": 0.99}"#.to_string(),
                tool_calls: None,
            })
        }
        async fn stream(
            &self,
            req: &CompletionRequest,
            _tx: tokio::sync::mpsc::Sender<abigail_capabilities::cognitive::StreamEvent>,
        ) -> anyhow::Result<abigail_capabilities::cognitive::CompletionResponse> {
            self.complete(req).await
        }
    }

    #[tokio::test]
    async fn test_l2_fallback_with_mock_llm() {
        let llm = Arc::new(MockClassifierLlm {
            response: r#"{"tier": "T3", "confidence": 0.9}"#.to_string(),
        });
        let c = PromptClassifier::new(Some(llm));
        // "Tell me about dogs" → L1 returns T2 with low confidence → triggers L2
        let r = c.classify("Tell me about dogs").await;
        assert_eq!(r.tier, PromptTier::T3Pro, "L2 should override L1 result");
        assert_eq!(r.matched_rule.as_deref(), Some("llm_layer2"));
    }

    #[tokio::test]
    async fn test_l2_fallback_timeout_uses_l1() {
        let llm = Arc::new(SlowClassifierLlm);
        let c = PromptClassifier::new(Some(llm));
        // SlowMock takes 5s, timeout is 3s → falls back to L1 result
        let r = c.classify("Tell me about dogs").await;
        assert_eq!(
            r.tier,
            PromptTier::T2Standard,
            "timeout should fall back to L1"
        );
    }

    #[tokio::test]
    async fn test_l2_fallback_error_uses_l1() {
        let llm = Arc::new(FailingClassifierLlm);
        let c = PromptClassifier::new(Some(llm));
        // FailingMock → falls back to L1 result
        let r = c.classify("Tell me about dogs").await;
        assert_eq!(
            r.tier,
            PromptTier::T2Standard,
            "error should fall back to L1"
        );
    }

    #[tokio::test]
    async fn test_overlapping_pattern_code_wins() {
        let c = classifier();
        // "write a function to analyze" contains both "write a function" (T4) and "analyze" (T3)
        // Code markers are checked before analysis markers → T4 should win
        let r = c.classify("write a function to analyze data").await;
        assert_eq!(r.tier, PromptTier::T4Specialist);
        assert_eq!(r.matched_rule.as_deref(), Some("code_request"));
    }

    #[tokio::test]
    async fn test_long_message_t3() {
        let c = classifier();
        // >100 words, no specific markers → T3Pro (long_message rule)
        let long_msg = "word ".repeat(120);
        let r = c.classify(&long_msg).await;
        assert_eq!(r.tier, PromptTier::T3Pro);
        assert_eq!(r.matched_rule.as_deref(), Some("long_message"));
    }

    #[tokio::test]
    async fn test_l2_parse_no_confidence() {
        let result = parse_l2_response(r#"{"tier": "T2"}"#);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.tier, PromptTier::T2Standard);
        // Default confidence when missing
        assert!((r.confidence - 0.7).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_l2_parse_invalid_tier() {
        let result = parse_l2_response(r#"{"tier": "T5"}"#);
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_l2_parse_empty_string() {
        let result = parse_l2_response("");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_tool_use_search_the_web() {
        let c = classifier();
        let r = c.classify("search the web for rust news").await;
        assert_eq!(r.tier, PromptTier::T3Pro);
        assert_eq!(r.matched_rule.as_deref(), Some("tool_use_request"));
    }

    #[tokio::test]
    async fn test_tool_use_read_file() {
        let c = classifier();
        let r = c.classify("read the file at C:\\Users\\test.txt").await;
        assert_eq!(r.tier, PromptTier::T3Pro);
        assert_eq!(r.matched_rule.as_deref(), Some("tool_use_request"));
    }

    #[tokio::test]
    async fn test_tool_use_send_email() {
        let c = classifier();
        let r = c.classify("send email to alice@example.com").await;
        assert_eq!(r.tier, PromptTier::T3Pro);
        assert_eq!(r.matched_rule.as_deref(), Some("tool_use_request"));
    }

    #[tokio::test]
    async fn test_tool_use_download() {
        let c = classifier();
        let r = c
            .classify("download the latest report from the server")
            .await;
        assert_eq!(r.tier, PromptTier::T3Pro);
        assert_eq!(r.matched_rule.as_deref(), Some("tool_use_request"));
    }

    // ── Decision matrix tests ─────────────────────────────────────────

    #[test]
    fn test_matrix_composite_default_weights() {
        let m = DecisionMatrix {
            complexity: 80,
            ethical_weight: 10,
            latency_tolerance: 70,
            tool_count: 30,
            user_preference: 50,
        };
        // 80*0.30 + 10*0.15 + 70*0.15 + 30*0.20 + 50*0.20
        // = 24 + 1.5 + 10.5 + 6 + 10 = 52
        assert_eq!(m.composite(), 52);
    }

    #[test]
    fn test_matrix_composite_custom_weights() {
        let m = DecisionMatrix {
            complexity: 100,
            ethical_weight: 0,
            latency_tolerance: 0,
            tool_count: 0,
            user_preference: 0,
        };
        let w = FactorWeights {
            complexity: 1.0,
            ethical_weight: 0.0,
            latency_tolerance: 0.0,
            tool_count: 0.0,
            user_preference: 0.0,
        };
        assert_eq!(m.composite_with_weights(&w), 100);
    }

    #[test]
    fn test_matrix_composite_clamped_to_100() {
        let m = DecisionMatrix {
            complexity: 100,
            ethical_weight: 100,
            latency_tolerance: 100,
            tool_count: 100,
            user_preference: 100,
        };
        assert_eq!(m.composite(), 100);
    }

    #[test]
    fn test_matrix_all_zeros() {
        let m = DecisionMatrix {
            complexity: 0,
            ethical_weight: 0,
            latency_tolerance: 0,
            tool_count: 0,
            user_preference: 0,
        };
        assert_eq!(m.composite(), 0);
    }

    #[test]
    fn test_matrix_advisory_tier() {
        let low = DecisionMatrix {
            complexity: 5,
            ethical_weight: 5,
            latency_tolerance: 5,
            tool_count: 0,
            user_preference: 10,
        };
        assert_eq!(low.advisory_tier(), PromptTier::T1Fast);

        let high = DecisionMatrix {
            complexity: 100,
            ethical_weight: 50,
            latency_tolerance: 100,
            tool_count: 90,
            user_preference: 90,
        };
        // 100*0.30 + 50*0.15 + 100*0.15 + 90*0.20 + 90*0.20 = 30+7.5+15+18+18 = 88.5 → 89
        assert_eq!(high.advisory_tier(), PromptTier::T4Specialist);
    }

    #[test]
    fn test_routing_target_from_score() {
        // Low composite → Id
        let m = DecisionMatrix {
            complexity: 5,
            ethical_weight: 5,
            latency_tolerance: 5,
            tool_count: 0,
            user_preference: 50,
        };
        assert_eq!(m.to_routing_target(), RoutingTarget::Id);

        // High composite → Ego
        let m = DecisionMatrix {
            complexity: 80,
            ethical_weight: 10,
            latency_tolerance: 80,
            tool_count: 60,
            user_preference: 50,
        };
        assert_eq!(m.to_routing_target(), RoutingTarget::Ego);
    }

    #[test]
    fn test_routing_target_superego_trigger() {
        // Ethical weight >= 80 → Superego regardless of composite
        let m = DecisionMatrix {
            complexity: 10,
            ethical_weight: 85,
            latency_tolerance: 10,
            tool_count: 0,
            user_preference: 50,
        };
        assert_eq!(m.to_routing_target(), RoutingTarget::Superego);
    }

    #[test]
    fn test_routing_target_display() {
        assert_eq!(RoutingTarget::Id.to_string(), "Id");
        assert_eq!(RoutingTarget::Ego.to_string(), "Ego");
        assert_eq!(RoutingTarget::Superego.to_string(), "Superego");
    }

    #[test]
    fn test_matrix_display_format() {
        let m = DecisionMatrix {
            complexity: 80,
            ethical_weight: 10,
            latency_tolerance: 70,
            tool_count: 30,
            user_preference: 50,
        };
        let s = m.to_string();
        assert!(s.contains("score=52"));
        assert!(s.contains("complexity=80"));
        assert!(s.contains("ethical=10"));
        assert!(s.contains("latency=70"));
        assert!(s.contains("tools=30"));
        assert!(s.contains("preference=50"));
    }

    #[tokio::test]
    async fn test_greeting_routes_to_id() {
        let c = classifier();
        let r = c.classify("hello").await;
        assert_eq!(r.routing_target, RoutingTarget::Id);
        assert!(r.matrix.complexity <= 10);
    }

    #[tokio::test]
    async fn test_code_request_routes_to_ego() {
        let c = classifier();
        let r = c.classify("Write a function to sort an array").await;
        assert_eq!(r.routing_target, RoutingTarget::Ego);
        assert!(r.matrix.complexity >= 30);
        assert!(r.matrix.latency_tolerance >= 60);
    }

    #[tokio::test]
    async fn test_standard_question_routes_to_id() {
        let c = classifier();
        let r = c.classify("Tell me about dogs").await;
        assert_eq!(r.routing_target, RoutingTarget::Id);
    }

    #[tokio::test]
    async fn test_complex_analysis_routes_to_ego() {
        let c = classifier();
        let r = c.classify("Analyze the pros and cons of remote work").await;
        assert_eq!(r.routing_target, RoutingTarget::Ego);
        assert!(r.matrix.complexity >= 30);
    }

    #[tokio::test]
    async fn test_ethical_weight_increases_for_sensitive_topics() {
        let c = classifier();

        let normal = c.classify("Tell me about dogs").await;
        let sensitive = c
            .classify("Give me medical advice about my diagnosis")
            .await;

        assert!(
            sensitive.matrix.ethical_weight > normal.matrix.ethical_weight,
            "sensitive topic should have higher ethical weight (got {} vs {})",
            sensitive.matrix.ethical_weight,
            normal.matrix.ethical_weight,
        );
    }

    #[tokio::test]
    async fn test_tool_count_factor() {
        let c = classifier();

        let no_tools = c.classify("Tell me about dogs").await;
        let one_tool = c.classify("search the web for rust news").await;
        let multi_tool = c
            .classify("search for the file and then send email with the results")
            .await;

        assert_eq!(no_tools.matrix.tool_count, 0);
        assert!(one_tool.matrix.tool_count > 0);
        assert!(multi_tool.matrix.tool_count > one_tool.matrix.tool_count);
    }

    #[tokio::test]
    async fn test_user_preference_fast_hint() {
        let c = classifier();
        let r = c.classify("briefly tell me about dogs").await;
        assert!(
            r.matrix.user_preference < 50,
            "fast hint should lower preference (got {})",
            r.matrix.user_preference
        );
    }

    #[tokio::test]
    async fn test_user_preference_quality_hint() {
        let c = classifier();
        let r = c
            .classify("give me a thorough explanation of quantum mechanics")
            .await;
        assert!(
            r.matrix.user_preference > 50,
            "quality hint should raise preference (got {})",
            r.matrix.user_preference
        );
    }

    #[tokio::test]
    async fn test_user_preference_neutral_default() {
        let c = classifier();
        let r = c.classify("Tell me about dogs").await;
        assert_eq!(r.matrix.user_preference, 50);
    }

    #[tokio::test]
    async fn test_with_custom_weights() {
        // Heavy complexity weight classifier
        let w = FactorWeights {
            complexity: 0.80,
            ethical_weight: 0.05,
            latency_tolerance: 0.05,
            tool_count: 0.05,
            user_preference: 0.05,
        };
        let c = PromptClassifier::with_weights(None, w);
        let r = c.classify("Write a function to sort an array").await;
        // Tier should still be T4 (signal-based), but composite score differs
        assert_eq!(r.tier, PromptTier::T4Specialist);
    }

    #[tokio::test]
    async fn test_matrix_present_on_l2_result() {
        let result = parse_l2_response(r#"{"tier": "T3", "confidence": 0.85}"#);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.routing_target, RoutingTarget::Ego);
        // L2 results have a neutral matrix
        assert_eq!(r.matrix.user_preference, 50);
    }

    #[tokio::test]
    async fn test_factor_scoring_latency_tolerance() {
        let c = classifier();

        let greeting = c.classify("hello").await;
        let code = c.classify("Write a function to sort arrays").await;
        let analysis = c.classify("Analyze the impact of climate change").await;

        assert!(
            greeting.matrix.latency_tolerance < code.matrix.latency_tolerance,
            "code should tolerate more latency than greeting"
        );
        assert!(
            analysis.matrix.latency_tolerance > greeting.matrix.latency_tolerance,
            "analysis should tolerate more latency than greeting"
        );
    }

    #[test]
    fn test_score_to_routing_target_boundary() {
        assert_eq!(score_to_routing_target(0), RoutingTarget::Id);
        assert_eq!(score_to_routing_target(30), RoutingTarget::Id);
        assert_eq!(score_to_routing_target(31), RoutingTarget::Ego);
        assert_eq!(score_to_routing_target(100), RoutingTarget::Ego);
    }

    #[test]
    fn test_tier_from_signals_priority() {
        // Code wins over analysis
        let mut s = Signals::default();
        s.is_code_request = true;
        s.is_complex_analysis = true;
        assert_eq!(tier_from_signals(&s), PromptTier::T4Specialist);

        // Analysis wins over greeting
        let mut s = Signals::default();
        s.is_complex_analysis = true;
        s.is_greeting = true;
        assert_eq!(tier_from_signals(&s), PromptTier::T3Pro);

        // No signals → T2Standard
        let s = Signals::default();
        assert_eq!(tier_from_signals(&s), PromptTier::T2Standard);
    }
}
