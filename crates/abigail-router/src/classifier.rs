//! Prompt complexity classifier for multi-tier routing.
//!
//! Two-layer classification:
//! - **Layer 1**: Deterministic keyword/pattern rules (fast, no LLM call)
//! - **Layer 2**: LLM-based fallback when Layer 1 confidence is low

use abigail_capabilities::cognitive::{CompletionRequest, LlmProvider, Message};
use std::sync::Arc;

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

/// Result of classifying a prompt.
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub tier: PromptTier,
    /// Confidence score 0.0–1.0
    pub confidence: f32,
    /// Which rule or method produced this classification (for diagnostics)
    pub matched_rule: Option<String>,
}

/// Minimum Layer 1 confidence before falling back to Layer 2 (LLM).
const L2_CONFIDENCE_THRESHOLD: f32 = 0.5;

/// Timeout for Layer 2 LLM classification (milliseconds).
const L2_TIMEOUT_MS: u64 = 3000;

/// Classifies prompts into complexity tiers.
pub struct PromptClassifier {
    /// Optional LLM for Layer 2 fallback classification.
    llm: Option<Arc<dyn LlmProvider>>,
}

impl PromptClassifier {
    /// Create a classifier with an optional LLM for Layer 2 fallback.
    pub fn new(llm: Option<Arc<dyn LlmProvider>>) -> Self {
        Self { llm }
    }

    /// Classify a user message into a PromptTier.
    pub async fn classify(&self, message: &str) -> ClassificationResult {
        let l1_result = self.classify_layer1(message);

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

    /// Layer 1: Deterministic rule-based classification (fast, no LLM).
    fn classify_layer1(&self, message: &str) -> ClassificationResult {
        let lower = message.trim().to_lowercase();
        let word_count = message.split_whitespace().count();

        // ── T1Fast: Greetings ──
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
                return ClassificationResult {
                    tier: PromptTier::T1Fast,
                    confidence: 0.95,
                    matched_rule: Some("greeting".to_string()),
                };
            }
        }

        // ── T1Fast: Very short, non-question messages ──
        if word_count <= 3
            && !lower.contains('?')
            && !lower.starts_with("write")
            && !lower.starts_with("explain")
            && !lower.starts_with("analyze")
        {
            return ClassificationResult {
                tier: PromptTier::T1Fast,
                confidence: 0.6,
                matched_rule: Some("short_message".to_string()),
            };
        }

        // ── T1Fast: Simple time/date/weather questions ──
        let simple_q_patterns = [
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
        for pat in &simple_q_patterns {
            if lower.contains(pat) {
                return ClassificationResult {
                    tier: PromptTier::T1Fast,
                    confidence: 0.9,
                    matched_rule: Some("simple_question".to_string()),
                };
            }
        }

        // ── T4Specialist: Code detection ──
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
                return ClassificationResult {
                    tier: PromptTier::T4Specialist,
                    confidence: 0.85,
                    matched_rule: Some("code_request".to_string()),
                };
            }
        }

        // Code fences or obvious code content
        if message.contains("```")
            || message.contains("fn ")
            || message.contains("def ")
            || message.contains("class ")
            || message.contains("function ")
            || message.contains("import ")
            || message.contains("#include")
        {
            return ClassificationResult {
                tier: PromptTier::T4Specialist,
                confidence: 0.8,
                matched_rule: Some("code_content".to_string()),
            };
        }

        // ── T4Specialist: Math detection ──
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
                return ClassificationResult {
                    tier: PromptTier::T4Specialist,
                    confidence: 0.8,
                    matched_rule: Some("math_request".to_string()),
                };
            }
        }

        // ── T3Pro: Complex analysis markers ──
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
                return ClassificationResult {
                    tier: PromptTier::T3Pro,
                    confidence: 0.8,
                    matched_rule: Some("complex_analysis".to_string()),
                };
            }
        }

        // ── T3Pro: Creative writing markers ──
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
                return ClassificationResult {
                    tier: PromptTier::T3Pro,
                    confidence: 0.75,
                    matched_rule: Some("creative_writing".to_string()),
                };
            }
        }

        // ── T3Pro: Long messages likely need more reasoning ──
        if word_count > 100 {
            return ClassificationResult {
                tier: PromptTier::T3Pro,
                confidence: 0.6,
                matched_rule: Some("long_message".to_string()),
            };
        }

        // ── Default: T2Standard ──
        ClassificationResult {
            tier: PromptTier::T2Standard,
            confidence: 0.4, // Low confidence → may trigger L2 if LLM available
            matched_rule: Some("default".to_string()),
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

    Some(ClassificationResult {
        tier,
        confidence,
        matched_rule: Some("llm_layer2".to_string()),
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
}
