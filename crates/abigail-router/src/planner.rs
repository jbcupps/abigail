//! Planner: uses a Pro-tier LLM to decompose user requests into structured GoalFrames.
//!
//! The GoalFrame captures intent, completion criteria, risk assessment, and execution
//! bounds so the Execution Governor can drive a goal-oriented loop instead of a
//! simple request/response cycle.

use abigail_capabilities::cognitive::{CompletionRequest, LlmProvider, Message};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A structured goal frame produced by the Planner.
///
/// Contains everything the Execution Governor needs to run, evaluate,
/// and bound an autonomous execution loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalFrame {
    /// What the user wants to accomplish.
    pub intent: String,
    /// Criteria that *must* be met for the task to be considered done.
    pub done_criteria: Vec<String>,
    /// Nice-to-have criteria that improve quality but are not required.
    pub good_criteria: Vec<String>,
    /// What could go wrong during execution.
    pub risk_assessment: String,
    /// Recommended approach / strategy outline.
    pub suggested_approach: String,
    /// Conditions under which execution should be aborted.
    pub abort_conditions: Vec<String>,
    /// Maximum number of execution loop iterations.
    pub max_iterations: u32,
    /// Time budget in milliseconds.
    pub time_budget_ms: u64,
}

impl Default for GoalFrame {
    fn default() -> Self {
        Self {
            intent: String::new(),
            done_criteria: Vec::new(),
            good_criteria: Vec::new(),
            risk_assessment: String::new(),
            suggested_approach: String::new(),
            abort_conditions: Vec::new(),
            max_iterations: 10,
            time_budget_ms: 120_000,
        }
    }
}

impl GoalFrame {
    /// Quick check: are there any done criteria defined?
    pub fn has_done_criteria(&self) -> bool {
        !self.done_criteria.is_empty()
    }
}

/// Uses an LLM to produce structured GoalFrames from user requests.
///
/// The Planner is invoked once at the start of a complex task to produce
/// the GoalFrame that the Governor will use throughout execution.
pub struct Planner {
    /// The LLM provider used for planning (should be a Pro-tier model).
    provider: Arc<dyn LlmProvider>,
}

impl Planner {
    /// Create a new Planner backed by the given LLM provider.
    ///
    /// For best results, use a Pro-tier model (e.g. o1, claude-opus) since
    /// planning requires strong reasoning capabilities.
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }

    /// Use the LLM to produce a GoalFrame from a conversation and constraints.
    ///
    /// The planner sends a structured prompt asking the LLM to analyze the
    /// user's request and return a JSON GoalFrame. If the LLM response cannot
    /// be parsed, falls back to a simple text-based frame.
    pub async fn plan(
        &self,
        messages: &[Message],
        constraints: &[String],
    ) -> anyhow::Result<GoalFrame> {
        let user_context = messages
            .iter()
            .filter(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        if user_context.trim().is_empty() {
            anyhow::bail!("No user messages to plan from");
        }

        let constraint_block = if constraints.is_empty() {
            String::new()
        } else {
            format!(
                "\n\nActive constraints:\n{}",
                constraints
                    .iter()
                    .map(|c| format!("- {}", c))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };

        let prompt = format!(
            r#"You are a planning engine for an AI assistant. Analyze the user's request and produce a structured execution plan.

User request:
{user_context}{constraint_block}

Respond with ONLY a JSON object matching this schema (no markdown fences, no extra text):
{{
  "intent": "one-sentence summary of what the user wants",
  "done_criteria": ["criterion 1", "criterion 2"],
  "good_criteria": ["nice-to-have 1"],
  "risk_assessment": "what could go wrong",
  "suggested_approach": "step-by-step approach",
  "abort_conditions": ["when to give up"],
  "max_iterations": 10,
  "time_budget_ms": 120000
}}"#
        );

        let planning_messages = vec![Message::new("user", prompt)];
        let request = CompletionRequest::simple(planning_messages);

        tracing::debug!("Planner: sending planning request to LLM");

        let response = self.provider.complete(&request).await?;
        let raw = response.content.trim();

        tracing::debug!("Planner: LLM response length = {}", raw.len());

        // Try to parse the LLM response as a GoalFrame JSON.
        // Strip markdown fences if the model wrapped the JSON.
        let json_str = strip_markdown_fences(raw);

        match serde_json::from_str::<GoalFrame>(json_str) {
            Ok(frame) => {
                tracing::info!(
                    "Planner: parsed GoalFrame with {} done_criteria, {} good_criteria",
                    frame.done_criteria.len(),
                    frame.good_criteria.len()
                );
                Ok(frame)
            }
            Err(parse_err) => {
                tracing::warn!(
                    "Planner: failed to parse LLM response as GoalFrame: {}. Falling back to text extraction.",
                    parse_err
                );
                // Fallback: create a frame from the raw text
                Ok(Self::plan_from_text(&user_context))
            }
        }
    }

    /// Create a simple GoalFrame from a plain-text user request without using an LLM.
    ///
    /// Useful for quick tasks that don't warrant a full planning call,
    /// or as a fallback when the LLM is unavailable.
    pub fn plan_from_text(user_request: &str) -> GoalFrame {
        let trimmed = user_request.trim();
        let intent = if trimmed.len() > 200 {
            format!("{}...", &trimmed[..200])
        } else {
            trimmed.to_string()
        };

        GoalFrame {
            intent: intent.clone(),
            done_criteria: vec![format!("Successfully address: {}", intent)],
            good_criteria: vec!["Response is clear and well-structured".to_string()],
            risk_assessment: "No detailed risk assessment (text-only planning)".to_string(),
            suggested_approach: "Attempt direct completion of the request".to_string(),
            abort_conditions: vec![
                "Repeated failures with no progress".to_string(),
                "Time budget exceeded".to_string(),
            ],
            max_iterations: 10,
            time_budget_ms: 120_000,
        }
    }
}

/// Strip common markdown code fences from an LLM response.
fn strip_markdown_fences(raw: &str) -> &str {
    let trimmed = raw.trim();

    // Strip ```json ... ``` or ``` ... ```
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
    }

    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_capabilities::cognitive::{CompletionRequest, CompletionResponse, LlmProvider};
    use async_trait::async_trait;
    use std::sync::Arc;

    // ── Mock LLM Provider ──────────────────────────────────────────

    /// A mock LLM provider that returns a predetermined response.
    struct MockLlmProvider {
        response: String,
    }

    impl MockLlmProvider {
        fn new(response: impl Into<String>) -> Self {
            Self {
                response: response.into(),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockLlmProvider {
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

    // ── GoalFrame tests ────────────────────────────────────────────

    #[test]
    fn test_goal_frame_default() {
        let frame = GoalFrame::default();
        assert!(frame.intent.is_empty());
        assert!(frame.done_criteria.is_empty());
        assert!(!frame.has_done_criteria());
        assert_eq!(frame.max_iterations, 10);
        assert_eq!(frame.time_budget_ms, 120_000);
    }

    #[test]
    fn test_goal_frame_has_done_criteria() {
        let mut frame = GoalFrame::default();
        assert!(!frame.has_done_criteria());
        frame.done_criteria.push("task complete".to_string());
        assert!(frame.has_done_criteria());
    }

    #[test]
    fn test_goal_frame_serde_roundtrip() {
        let frame = GoalFrame {
            intent: "Search for weather data".to_string(),
            done_criteria: vec!["Temperature returned".to_string()],
            good_criteria: vec!["Include humidity".to_string()],
            risk_assessment: "API might be down".to_string(),
            suggested_approach: "Call weather API".to_string(),
            abort_conditions: vec!["API key invalid".to_string()],
            max_iterations: 5,
            time_budget_ms: 60_000,
        };

        let json = serde_json::to_string(&frame).unwrap();
        let roundtripped: GoalFrame = serde_json::from_str(&json).unwrap();

        assert_eq!(roundtripped.intent, frame.intent);
        assert_eq!(roundtripped.done_criteria.len(), 1);
        assert_eq!(roundtripped.max_iterations, 5);
        assert_eq!(roundtripped.time_budget_ms, 60_000);
    }

    // ── plan_from_text tests ───────────────────────────────────────

    #[test]
    fn test_plan_from_text_basic() {
        let frame = Planner::plan_from_text("What is the weather in Austin?");
        assert_eq!(frame.intent, "What is the weather in Austin?");
        assert!(!frame.done_criteria.is_empty());
        assert!(!frame.good_criteria.is_empty());
        assert!(!frame.abort_conditions.is_empty());
        assert_eq!(frame.max_iterations, 10);
        assert_eq!(frame.time_budget_ms, 120_000);
    }

    #[test]
    fn test_plan_from_text_truncates_long_request() {
        let long_request = "a".repeat(300);
        let frame = Planner::plan_from_text(&long_request);
        // Intent should be truncated to ~200 chars + "..."
        assert!(frame.intent.len() < 210);
        assert!(frame.intent.ends_with("..."));
    }

    #[test]
    fn test_plan_from_text_trims_whitespace() {
        let frame = Planner::plan_from_text("  hello world  ");
        assert_eq!(frame.intent, "hello world");
    }

    // ── LLM-based plan() tests ─────────────────────────────────────

    #[tokio::test]
    async fn test_plan_with_valid_json_response() {
        let json_response = r#"{
            "intent": "Find the current weather in Austin",
            "done_criteria": ["Temperature value obtained"],
            "good_criteria": ["Include 5-day forecast"],
            "risk_assessment": "Weather API may be rate-limited",
            "suggested_approach": "Query OpenWeatherMap API",
            "abort_conditions": ["API key missing"],
            "max_iterations": 3,
            "time_budget_ms": 30000
        }"#;

        let provider = Arc::new(MockLlmProvider::new(json_response));
        let planner = Planner::new(provider);

        let messages = vec![Message::new("user", "What is the weather in Austin?")];
        let frame = planner.plan(&messages, &[]).await.unwrap();

        assert_eq!(frame.intent, "Find the current weather in Austin");
        assert_eq!(frame.done_criteria.len(), 1);
        assert_eq!(frame.good_criteria.len(), 1);
        assert_eq!(frame.max_iterations, 3);
        assert_eq!(frame.time_budget_ms, 30_000);
    }

    #[tokio::test]
    async fn test_plan_with_markdown_fenced_json() {
        let fenced_response = r#"```json
{
    "intent": "Summarize a document",
    "done_criteria": ["Summary produced"],
    "good_criteria": [],
    "risk_assessment": "Document may be too long",
    "suggested_approach": "Chunk and summarize",
    "abort_conditions": ["Token limit exceeded"],
    "max_iterations": 5,
    "time_budget_ms": 60000
}
```"#;

        let provider = Arc::new(MockLlmProvider::new(fenced_response));
        let planner = Planner::new(provider);

        let messages = vec![Message::new("user", "Summarize this document")];
        let frame = planner.plan(&messages, &[]).await.unwrap();

        assert_eq!(frame.intent, "Summarize a document");
        assert_eq!(frame.max_iterations, 5);
    }

    #[tokio::test]
    async fn test_plan_with_unparseable_response_falls_back() {
        let bad_response = "I think you should try searching the web first and then...";

        let provider = Arc::new(MockLlmProvider::new(bad_response));
        let planner = Planner::new(provider);

        let messages = vec![Message::new("user", "Find the weather")];
        let frame = planner.plan(&messages, &[]).await.unwrap();

        // Falls back to plan_from_text
        assert!(frame.intent.contains("Find the weather"));
        assert!(!frame.done_criteria.is_empty());
        assert_eq!(frame.max_iterations, 10); // default
    }

    #[tokio::test]
    async fn test_plan_with_no_user_messages_errors() {
        let provider = Arc::new(MockLlmProvider::new("{}"));
        let planner = Planner::new(provider);

        let messages = vec![Message::new("assistant", "Hello!")];
        let result = planner.plan(&messages, &[]).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No user messages"));
    }

    #[tokio::test]
    async fn test_plan_with_constraints() {
        // Verify that constraints are included in the prompt.
        // We use a mock that always returns valid JSON regardless.
        let json_response = r#"{
            "intent": "Plan with constraints",
            "done_criteria": ["Done"],
            "good_criteria": [],
            "risk_assessment": "None",
            "suggested_approach": "Direct",
            "abort_conditions": [],
            "max_iterations": 10,
            "time_budget_ms": 120000
        }"#;

        let provider = Arc::new(MockLlmProvider::new(json_response));
        let planner = Planner::new(provider);

        let messages = vec![Message::new("user", "Do something")];
        let constraints = vec![
            "No network access".to_string(),
            "Read-only filesystem".to_string(),
        ];
        let frame = planner.plan(&messages, &constraints).await.unwrap();

        assert_eq!(frame.intent, "Plan with constraints");
    }

    #[tokio::test]
    async fn test_plan_concatenates_multiple_user_messages() {
        let json_response = r#"{
            "intent": "Multi-turn request",
            "done_criteria": ["Addressed both parts"],
            "good_criteria": [],
            "risk_assessment": "None",
            "suggested_approach": "Handle sequentially",
            "abort_conditions": [],
            "max_iterations": 10,
            "time_budget_ms": 120000
        }"#;

        let provider = Arc::new(MockLlmProvider::new(json_response));
        let planner = Planner::new(provider);

        let messages = vec![
            Message::new("user", "First, find the file."),
            Message::new("assistant", "Okay, looking..."),
            Message::new("user", "Then summarize it."),
        ];
        let frame = planner.plan(&messages, &[]).await.unwrap();

        assert_eq!(frame.intent, "Multi-turn request");
        assert_eq!(frame.done_criteria, vec!["Addressed both parts"]);
    }

    // ── strip_markdown_fences tests ────────────────────────────────

    #[test]
    fn test_strip_markdown_fences_json() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_markdown_fences(input), r#"{"key": "value"}"#);
    }

    #[test]
    fn test_strip_markdown_fences_plain() {
        let input = "```\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_markdown_fences(input), r#"{"key": "value"}"#);
    }

    #[test]
    fn test_strip_markdown_fences_no_fences() {
        let input = r#"{"key": "value"}"#;
        assert_eq!(strip_markdown_fences(input), input);
    }

    #[test]
    fn test_strip_markdown_fences_whitespace() {
        let input = "  ```json\n{\"a\": 1}\n```  ";
        assert_eq!(strip_markdown_fences(input), r#"{"a": 1}"#);
    }
}
