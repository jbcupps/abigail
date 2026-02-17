//! Pro Council Engine -- Mixture-of-Agents deliberation.
//!
//! Implements a three-phase council protocol:
//! 1. **Draft**: each provider generates an independent response in parallel.
//! 2. **Critique**: providers cross-review each other's drafts with JSON 0-10 scoring.
//! 3. **Synthesis**: the first provider merges the ranked drafts into a final answer.

use abigail_capabilities::cognitive::provider::{CompletionRequest, LlmProvider, Message};
use std::sync::Arc;
use std::time::Duration;

/// A single draft produced by one council member.
#[derive(Debug, Clone)]
pub struct CouncilDraft {
    /// Name of the provider that produced this draft.
    pub provider: String,
    /// The draft content.
    pub content: String,
    /// Average score assigned during the critique phase (0-10).
    pub score: Option<f32>,
}

/// The outcome of a full council deliberation.
#[derive(Debug, Clone)]
pub struct CouncilResult {
    /// Final synthesized answer.
    pub synthesis: String,
    /// Individual drafts with their scores (sorted by score descending after critique).
    pub drafts: Vec<CouncilDraft>,
    /// Number of providers that participated.
    pub provider_count: usize,
}

/// Orchestrates multi-provider deliberation through draft, critique, and synthesis phases.
pub struct CouncilEngine {
    providers: Vec<(String, Arc<dyn LlmProvider>)>,
    timeout: Duration,
}

impl CouncilEngine {
    /// Create a new council engine with the given named providers.
    pub fn new(providers: Vec<(String, Arc<dyn LlmProvider>)>) -> Self {
        Self {
            providers,
            timeout: Duration::from_secs(90),
        }
    }

    /// Return the number of providers enrolled in this council.
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Set the overall time budget for the deliberation.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Run all three deliberation phases and return the synthesized result.
    ///
    /// Falls back gracefully when fewer than 2 providers are available:
    /// - 0 providers: returns an error.
    /// - 1 provider: returns its response directly (no critique or synthesis).
    /// - 2+ providers: full draft -> critique -> synthesis pipeline.
    pub async fn deliberate(
        &self,
        messages: Vec<Message>,
        system_context: Option<&str>,
    ) -> anyhow::Result<CouncilResult> {
        if self.providers.is_empty() {
            anyhow::bail!("CouncilEngine requires at least one provider");
        }

        // Wrap the entire pipeline in a timeout.
        let result =
            tokio::time::timeout(self.timeout, self.run_pipeline(messages, system_context))
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "Council deliberation timed out after {}s",
                        self.timeout.as_secs()
                    )
                })??;

        Ok(result)
    }

    /// Internal pipeline: draft -> critique -> synthesis.
    async fn run_pipeline(
        &self,
        messages: Vec<Message>,
        system_context: Option<&str>,
    ) -> anyhow::Result<CouncilResult> {
        let provider_count = self.providers.len();

        // ── Single provider fast-path ──────────────────────────────────
        if provider_count == 1 {
            let (name, provider) = &self.providers[0];
            let mut request_messages = Vec::new();
            if let Some(ctx) = system_context {
                request_messages.push(Message::new("system", ctx));
            }
            request_messages.extend(messages);

            let request = CompletionRequest::simple(request_messages);
            let response = provider.complete(&request).await?;

            let draft = CouncilDraft {
                provider: name.clone(),
                content: response.content.clone(),
                score: None,
            };

            return Ok(CouncilResult {
                synthesis: response.content,
                drafts: vec![draft],
                provider_count: 1,
            });
        }

        // ── Phase 1: Draft ─────────────────────────────────────────────
        tracing::info!(
            "Council: starting draft phase with {} providers",
            provider_count
        );
        let mut drafts = self.phase_draft(&messages, system_context).await?;

        // ── Phase 2: Critique ──────────────────────────────────────────
        tracing::info!("Council: starting critique phase");
        self.phase_critique(&mut drafts, &messages, system_context)
            .await;

        // Sort by score descending (highest first). Unscored drafts go last.
        drafts.sort_by(|a, b| {
            let sa = a.score.unwrap_or(-1.0);
            let sb = b.score.unwrap_or(-1.0);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });

        // ── Phase 3: Synthesis ─────────────────────────────────────────
        tracing::info!("Council: starting synthesis phase");
        let synthesis = self
            .phase_synthesis(&drafts, &messages, system_context)
            .await?;

        Ok(CouncilResult {
            synthesis,
            drafts,
            provider_count,
        })
    }

    /// Phase 1: each provider generates an independent draft in parallel.
    async fn phase_draft(
        &self,
        messages: &[Message],
        system_context: Option<&str>,
    ) -> anyhow::Result<Vec<CouncilDraft>> {
        let mut handles = Vec::new();

        for (name, provider) in &self.providers {
            let name = name.clone();
            let provider = Arc::clone(provider);
            let mut request_messages = Vec::new();
            if let Some(ctx) = system_context {
                request_messages.push(Message::new("system", ctx));
            }
            request_messages.extend(messages.iter().cloned());

            let handle = tokio::spawn(async move {
                let request = CompletionRequest::simple(request_messages);
                match provider.complete(&request).await {
                    Ok(response) => Some(CouncilDraft {
                        provider: name,
                        content: response.content,
                        score: None,
                    }),
                    Err(e) => {
                        tracing::warn!("Council draft failed for provider {}: {}", name, e);
                        None
                    }
                }
            });

            handles.push(handle);
        }

        let mut drafts = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Some(draft)) => drafts.push(draft),
                Ok(None) => {} // provider failed, already logged
                Err(e) => {
                    tracing::warn!("Council draft task panicked: {}", e);
                }
            }
        }

        if drafts.is_empty() {
            anyhow::bail!("All providers failed during the draft phase");
        }

        Ok(drafts)
    }

    /// Phase 2: cross-provider critique with JSON 0-10 scoring.
    ///
    /// Each provider reviews every draft that is NOT its own and assigns a score.
    /// The final score for each draft is the average across all reviewers.
    /// Critique failures are non-fatal -- drafts simply remain unscored.
    async fn phase_critique(
        &self,
        drafts: &mut [CouncilDraft],
        messages: &[Message],
        system_context: Option<&str>,
    ) {
        if drafts.len() < 2 {
            return;
        }

        // Build the original question text for critique context.
        let original_question: String = messages
            .iter()
            .filter(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Collect all critique tasks: (draft_index, join handle returning Option<f32>)
        let mut handles: Vec<(usize, tokio::task::JoinHandle<Option<f32>>)> = Vec::new();

        for (draft_idx, draft) in drafts.iter().enumerate() {
            for (reviewer_name, reviewer) in &self.providers {
                // Skip self-review.
                if *reviewer_name == draft.provider {
                    continue;
                }

                let reviewer = Arc::clone(reviewer);
                let draft_content = draft.content.clone();
                let draft_provider = draft.provider.clone();
                let question = original_question.clone();
                let ctx = system_context.map(String::from);

                let handle = tokio::spawn(async move {
                    let critique_prompt = format!(
                        "You are a critical reviewer. The user asked:\n\"{question}\"\n\n\
                         Provider \"{draft_provider}\" answered:\n\"{draft_content}\"\n\n\
                         Score this answer from 0 to 10 on accuracy, completeness, and helpfulness.\n\
                         Respond with ONLY a JSON object: {{\"score\": <number>}}\n\
                         Do not include any other text."
                    );

                    let mut req_messages = Vec::new();
                    if let Some(c) = ctx {
                        req_messages.push(Message::new("system", c));
                    }
                    req_messages.push(Message::new("user", critique_prompt));

                    let request = CompletionRequest::simple(req_messages);
                    match reviewer.complete(&request).await {
                        Ok(response) => parse_score(&response.content),
                        Err(e) => {
                            tracing::warn!("Critique request failed: {}", e);
                            None
                        }
                    }
                });

                handles.push((draft_idx, handle));
            }
        }

        // Accumulate scores per draft.
        let mut score_accum: Vec<Vec<f32>> = vec![Vec::new(); drafts.len()];

        for (draft_idx, handle) in handles {
            match handle.await {
                Ok(Some(score)) => score_accum[draft_idx].push(score),
                Ok(None) => {} // parse failed, already logged
                Err(e) => {
                    tracing::warn!("Critique task panicked: {}", e);
                }
            }
        }

        // Compute averages.
        for (idx, scores) in score_accum.iter().enumerate() {
            if !scores.is_empty() {
                let avg = scores.iter().sum::<f32>() / scores.len() as f32;
                drafts[idx].score = Some(avg);
                tracing::debug!(
                    "Council: draft from '{}' scored {:.1} (from {} reviews)",
                    drafts[idx].provider,
                    avg,
                    scores.len()
                );
            }
        }
    }

    /// Phase 3: the first provider synthesizes all ranked drafts into a final answer.
    async fn phase_synthesis(
        &self,
        drafts: &[CouncilDraft],
        messages: &[Message],
        system_context: Option<&str>,
    ) -> anyhow::Result<String> {
        let (_, synthesizer) = &self.providers[0];

        let original_question: String = messages
            .iter()
            .filter(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Build a summary of all drafts with their scores.
        let mut draft_summary = String::new();
        for (i, draft) in drafts.iter().enumerate() {
            let score_str = draft
                .score
                .map(|s| format!("{:.1}/10", s))
                .unwrap_or_else(|| "unscored".to_string());
            draft_summary.push_str(&format!(
                "--- Draft {} (from '{}', score: {}) ---\n{}\n\n",
                i + 1,
                draft.provider,
                score_str,
                draft.content,
            ));
        }

        let synthesis_prompt = format!(
            "You are a synthesis expert. The user asked:\n\"{original_question}\"\n\n\
             Multiple experts have provided answers, ranked by peer review score \
             (highest first):\n\n{draft_summary}\
             Produce a single, authoritative answer that combines the best elements \
             from the top-scoring drafts. Be concise and accurate. \
             Do not mention the drafts or the scoring process in your answer."
        );

        let mut request_messages = Vec::new();
        if let Some(ctx) = system_context {
            request_messages.push(Message::new("system", ctx));
        }
        request_messages.push(Message::new("user", synthesis_prompt));

        let request = CompletionRequest::simple(request_messages);
        let response = synthesizer.complete(&request).await?;

        Ok(response.content)
    }
}

/// Parse a score from LLM output that should contain `{"score": <number>}`.
///
/// Tolerant of extra text around the JSON -- scans for the first `{` and last `}`.
fn parse_score(text: &str) -> Option<f32> {
    let trimmed = text.trim();

    // Try to find JSON object boundaries.
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end <= start {
        return None;
    }

    let json_str = &trimmed[start..=end];
    let parsed: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let score = parsed.get("score")?.as_f64()? as f32;

    // Clamp to valid range.
    Some(score.clamp(0.0, 10.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_capabilities::cognitive::provider::CompletionResponse;
    use async_trait::async_trait;

    // ── Mock providers ─────────────────────────────────────────────────

    /// A deterministic mock LLM provider for testing.
    #[derive(Clone)]
    struct MockProvider {
        name: String,
        response: String,
    }

    impl MockProvider {
        fn new(name: &str, response: &str) -> Self {
            Self {
                name: name.to_string(),
                response: response.to_string(),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
            Ok(CompletionResponse {
                content: self.response.clone(),
                tool_calls: None,
            })
        }
    }

    /// A mock that returns a draft normally and a JSON score for critique prompts.
    #[derive(Clone)]
    struct ScoringMockProvider {
        draft_response: String,
        score: f32,
    }

    impl ScoringMockProvider {
        fn new(draft_response: &str, score: f32) -> Self {
            Self {
                draft_response: draft_response.to_string(),
                score,
            }
        }
    }

    #[async_trait]
    impl LlmProvider for ScoringMockProvider {
        async fn complete(
            &self,
            request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
            // If the prompt looks like a critique request, return a score.
            let last_content = request
                .messages
                .last()
                .map(|m| m.content.as_str())
                .unwrap_or("");
            if last_content.contains("Score this answer") {
                Ok(CompletionResponse {
                    content: format!("{{\"score\": {}}}", self.score),
                    tool_calls: None,
                })
            } else {
                Ok(CompletionResponse {
                    content: self.draft_response.clone(),
                    tool_calls: None,
                })
            }
        }
    }

    /// A mock that always fails.
    #[derive(Clone)]
    struct FailingProvider;

    #[async_trait]
    impl LlmProvider for FailingProvider {
        async fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
            anyhow::bail!("Simulated provider failure")
        }
    }

    // ── parse_score tests ──────────────────────────────────────────────

    #[test]
    fn test_parse_score_valid_json() {
        assert_eq!(parse_score(r#"{"score": 7.5}"#), Some(7.5));
    }

    #[test]
    fn test_parse_score_with_surrounding_text() {
        assert_eq!(
            parse_score(r#"Here is my review: {"score": 8} - good work"#),
            Some(8.0)
        );
    }

    #[test]
    fn test_parse_score_integer() {
        assert_eq!(parse_score(r#"{"score": 10}"#), Some(10.0));
    }

    #[test]
    fn test_parse_score_zero() {
        assert_eq!(parse_score(r#"{"score": 0}"#), Some(0.0));
    }

    #[test]
    fn test_parse_score_out_of_range_clamped_high() {
        assert_eq!(parse_score(r#"{"score": 15}"#), Some(10.0));
    }

    #[test]
    fn test_parse_score_out_of_range_clamped_low() {
        assert_eq!(parse_score(r#"{"score": -3}"#), Some(0.0));
    }

    #[test]
    fn test_parse_score_invalid_json() {
        assert_eq!(parse_score("not json at all"), None);
    }

    #[test]
    fn test_parse_score_missing_key() {
        assert_eq!(parse_score(r#"{"rating": 5}"#), None);
    }

    #[test]
    fn test_parse_score_empty() {
        assert_eq!(parse_score(""), None);
    }

    #[test]
    fn test_parse_score_nested_braces() {
        // Should find the outermost braces and parse.
        assert_eq!(parse_score(r#"{"score": 6, "reason": "ok"}"#), Some(6.0));
    }

    #[test]
    fn test_parse_score_non_numeric() {
        assert_eq!(parse_score(r#"{"score": "high"}"#), None);
    }

    // ── CouncilEngine construction tests ───────────────────────────────

    #[test]
    fn test_council_default_timeout() {
        let engine = CouncilEngine::new(vec![]);
        assert_eq!(engine.timeout, Duration::from_secs(90));
    }

    #[test]
    fn test_council_custom_timeout() {
        let engine = CouncilEngine::new(vec![]).with_timeout(Duration::from_secs(30));
        assert_eq!(engine.timeout, Duration::from_secs(30));
    }

    // ── CouncilEngine::deliberate tests ────────────────────────────────

    #[tokio::test]
    async fn test_council_no_providers() {
        let engine = CouncilEngine::new(vec![]);
        let result = engine
            .deliberate(vec![Message::new("user", "hello")], None)
            .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("at least one provider"));
    }

    #[tokio::test]
    async fn test_council_single_provider() {
        let provider = Arc::new(MockProvider::new("alpha", "The answer is 42."));
        let engine = CouncilEngine::new(vec![("alpha".to_string(), provider)]);

        let result = engine
            .deliberate(
                vec![Message::new("user", "What is the meaning of life?")],
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.provider_count, 1);
        assert_eq!(result.drafts.len(), 1);
        assert_eq!(result.drafts[0].provider, "alpha");
        assert_eq!(result.synthesis, "The answer is 42.");
        // Single provider skips critique, so no score.
        assert!(result.drafts[0].score.is_none());
    }

    #[tokio::test]
    async fn test_council_single_provider_with_system_context() {
        let provider = Arc::new(MockProvider::new("alpha", "Context-aware response."));
        let engine = CouncilEngine::new(vec![("alpha".to_string(), provider)]);

        let result = engine
            .deliberate(
                vec![Message::new("user", "hello")],
                Some("You are a helpful assistant."),
            )
            .await
            .unwrap();

        assert_eq!(result.synthesis, "Context-aware response.");
        assert_eq!(result.provider_count, 1);
    }

    #[tokio::test]
    async fn test_council_multi_provider_deliberation() {
        let p1 = Arc::new(ScoringMockProvider::new("Draft from alpha", 8.0));
        let p2 = Arc::new(ScoringMockProvider::new("Draft from beta", 6.0));

        let providers: Vec<(String, Arc<dyn LlmProvider>)> = vec![
            ("alpha".to_string(), p1 as Arc<dyn LlmProvider>),
            ("beta".to_string(), p2 as Arc<dyn LlmProvider>),
        ];

        let engine = CouncilEngine::new(providers);
        let result = engine
            .deliberate(vec![Message::new("user", "Explain gravity")], None)
            .await
            .unwrap();

        assert_eq!(result.provider_count, 2);
        assert_eq!(result.drafts.len(), 2);
        // Synthesis should have been produced by the first provider.
        assert!(!result.synthesis.is_empty());
    }

    #[tokio::test]
    async fn test_council_multi_provider_drafts_are_scored() {
        let p1 = Arc::new(ScoringMockProvider::new("Alpha answer", 9.0));
        let p2 = Arc::new(ScoringMockProvider::new("Beta answer", 9.0));

        let providers: Vec<(String, Arc<dyn LlmProvider>)> = vec![
            ("alpha".to_string(), p1 as Arc<dyn LlmProvider>),
            ("beta".to_string(), p2 as Arc<dyn LlmProvider>),
        ];

        let engine = CouncilEngine::new(providers);
        let result = engine
            .deliberate(vec![Message::new("user", "test")], None)
            .await
            .unwrap();

        // Both drafts should be scored (each reviewed by the other).
        for draft in &result.drafts {
            assert!(
                draft.score.is_some(),
                "Draft from '{}' should be scored",
                draft.provider
            );
        }
    }

    #[tokio::test]
    async fn test_council_drafts_sorted_descending_by_score() {
        let p1 = Arc::new(ScoringMockProvider::new("Answer A", 7.0));
        let p2 = Arc::new(ScoringMockProvider::new("Answer B", 9.0));
        let p3 = Arc::new(ScoringMockProvider::new("Answer C", 5.0));

        let providers: Vec<(String, Arc<dyn LlmProvider>)> = vec![
            ("a".to_string(), p1 as Arc<dyn LlmProvider>),
            ("b".to_string(), p2 as Arc<dyn LlmProvider>),
            ("c".to_string(), p3 as Arc<dyn LlmProvider>),
        ];

        let engine = CouncilEngine::new(providers);
        let result = engine
            .deliberate(vec![Message::new("user", "test query")], None)
            .await
            .unwrap();

        assert_eq!(result.provider_count, 3);
        assert_eq!(result.drafts.len(), 3);

        // Drafts should be sorted by score descending.
        let scores: Vec<f32> = result.drafts.iter().filter_map(|d| d.score).collect();
        for window in scores.windows(2) {
            assert!(
                window[0] >= window[1],
                "Drafts should be sorted descending by score, got {:?}",
                scores
            );
        }
    }

    #[tokio::test]
    async fn test_council_partial_failure_in_draft() {
        // One provider succeeds, one fails -- should still produce a result
        // via the single-draft fallback (no critique needed with one draft).
        let good = Arc::new(MockProvider::new("good", "I have an answer."));
        let bad = Arc::new(FailingProvider) as Arc<dyn LlmProvider>;

        let providers: Vec<(String, Arc<dyn LlmProvider>)> = vec![
            ("good".to_string(), good as Arc<dyn LlmProvider>),
            ("bad".to_string(), bad),
        ];

        let engine = CouncilEngine::new(providers);
        let result = engine
            .deliberate(vec![Message::new("user", "hello")], None)
            .await
            .unwrap();

        // Only the good provider's draft should be present.
        assert_eq!(result.drafts.len(), 1);
        assert_eq!(result.drafts[0].provider, "good");
    }

    #[tokio::test]
    async fn test_council_all_providers_fail_in_draft() {
        let bad1 = Arc::new(FailingProvider) as Arc<dyn LlmProvider>;
        let bad2 = Arc::new(FailingProvider) as Arc<dyn LlmProvider>;

        let providers: Vec<(String, Arc<dyn LlmProvider>)> =
            vec![("bad1".to_string(), bad1), ("bad2".to_string(), bad2)];

        let engine = CouncilEngine::new(providers);
        let result = engine
            .deliberate(vec![Message::new("user", "hello")], None)
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("All providers failed"));
    }

    #[tokio::test]
    async fn test_council_timeout() {
        /// A provider that sleeps indefinitely.
        #[derive(Clone)]
        struct SlowProvider;

        #[async_trait]
        impl LlmProvider for SlowProvider {
            async fn complete(
                &self,
                _request: &CompletionRequest,
            ) -> anyhow::Result<CompletionResponse> {
                tokio::time::sleep(Duration::from_secs(600)).await;
                Ok(CompletionResponse {
                    content: "too late".to_string(),
                    tool_calls: None,
                })
            }
        }

        let provider = Arc::new(SlowProvider) as Arc<dyn LlmProvider>;
        let engine = CouncilEngine::new(vec![("slow".to_string(), provider)])
            .with_timeout(Duration::from_millis(50));

        let result = engine
            .deliberate(vec![Message::new("user", "hello")], None)
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn test_council_with_system_context_multi_provider() {
        let p1 = Arc::new(ScoringMockProvider::new("System-aware A", 8.0));
        let p2 = Arc::new(ScoringMockProvider::new("System-aware B", 7.0));

        let providers: Vec<(String, Arc<dyn LlmProvider>)> = vec![
            ("a".to_string(), p1 as Arc<dyn LlmProvider>),
            ("b".to_string(), p2 as Arc<dyn LlmProvider>),
        ];

        let engine = CouncilEngine::new(providers);
        let result = engine
            .deliberate(vec![Message::new("user", "test")], Some("Be very precise."))
            .await
            .unwrap();

        assert_eq!(result.provider_count, 2);
        assert!(!result.synthesis.is_empty());
    }

    #[tokio::test]
    async fn test_council_multiple_user_messages() {
        let provider = Arc::new(MockProvider::new("alpha", "Combined answer."));
        let engine = CouncilEngine::new(vec![("alpha".to_string(), provider)]);

        let messages = vec![
            Message::new("user", "First question."),
            Message::new("assistant", "First answer."),
            Message::new("user", "Follow-up question."),
        ];

        let result = engine.deliberate(messages, None).await.unwrap();
        assert_eq!(result.synthesis, "Combined answer.");
    }

    #[tokio::test]
    async fn test_council_result_fields() {
        let p1 = Arc::new(ScoringMockProvider::new("Draft 1", 7.0));
        let p2 = Arc::new(ScoringMockProvider::new("Draft 2", 8.0));

        let providers: Vec<(String, Arc<dyn LlmProvider>)> = vec![
            ("first".to_string(), p1 as Arc<dyn LlmProvider>),
            ("second".to_string(), p2 as Arc<dyn LlmProvider>),
        ];

        let engine = CouncilEngine::new(providers);
        let result = engine
            .deliberate(vec![Message::new("user", "query")], None)
            .await
            .unwrap();

        // Verify CouncilResult structure.
        assert_eq!(result.provider_count, 2);
        assert!(!result.synthesis.is_empty());
        assert_eq!(result.drafts.len(), 2);

        // Verify each draft has the expected fields.
        for draft in &result.drafts {
            assert!(!draft.provider.is_empty());
            assert!(!draft.content.is_empty());
        }
    }
}
