//! Superego evaluator — content evaluation and context-aware gating policy.
//!
//! Two layers of judgment:
//! 1. **Pattern evaluation** (synchronous, free): PII, destructive intent, and
//!    leaked-secret detection. Used as the pre-delivery gate by the Ego stage
//!    and as the first-pass verdict by the superego pipeline stage.
//! 2. **LLM judgment** (async, local Id provider): prompt/verdict helpers for
//!    judging an exchange against the entity's signed constitution. The caller
//!    owns the model call; this module owns the prompt and verdict parsing.
//!
//! Gating policy is context-aware: *talking about* a destructive command in a
//! chat reply is legitimate (a parent asking what `rm -rf` does deserves an
//! answer), but a reply that leaks an API key, or a tool/job payload that
//! carries destructive intent, gets blocked.

use serde::{Deserialize, Serialize};

/// Where the evaluated content sits in the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationContext {
    /// Text the entity is about to show the mentor.
    ChatReply,
    /// Parameters or commands a tool is about to execute.
    ToolExecution,
    /// Output produced by a background job / sub-agent.
    JobResult,
    /// A skill event payload.
    SkillEvent,
}

impl EvaluationContext {
    pub fn as_str(&self) -> &'static str {
        match self {
            EvaluationContext::ChatReply => "chat_reply",
            EvaluationContext::ToolExecution => "tool_execution",
            EvaluationContext::JobResult => "job_result",
            EvaluationContext::SkillEvent => "skill_event",
        }
    }
}

/// Severity of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

/// A single concern identified in evaluated content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuperegoFinding {
    /// Stable rule identifier (e.g. "destructive-pattern", "secret-leak").
    pub rule: String,
    pub severity: Severity,
    pub description: String,
}

/// Gate decision for content in a given context.
#[derive(Debug, Clone)]
pub enum GateDecision {
    /// Nothing of concern.
    Allow,
    /// Concerns noted; content proceeds with an audit trail.
    Flag(Vec<SuperegoFinding>),
    /// Content must not proceed.
    Block(Vec<SuperegoFinding>),
}

impl GateDecision {
    pub fn verdict(&self) -> &'static str {
        match self {
            GateDecision::Allow => "clear",
            GateDecision::Flag(_) => "flag",
            GateDecision::Block(_) => "block",
        }
    }

    pub fn findings(&self) -> &[SuperegoFinding] {
        match self {
            GateDecision::Allow => &[],
            GateDecision::Flag(f) | GateDecision::Block(f) => f,
        }
    }
}

const DESTRUCTIVE_KEYWORDS: &[&str] = &[
    "drop table",
    "delete all",
    "rm -rf",
    "format c:",
    "wipe all data",
    "truncate table",
    "delete database",
];

/// Evaluate content and apply the context-aware gating policy.
pub fn evaluate(content: &str, context: EvaluationContext) -> GateDecision {
    let findings = find_concerns(content);
    if findings.is_empty() {
        return GateDecision::Allow;
    }

    let blocking: Vec<SuperegoFinding> = findings
        .iter()
        .filter(|f| f.severity == Severity::Critical && blocks_in_context(&f.rule, context))
        .cloned()
        .collect();

    if !blocking.is_empty() {
        GateDecision::Block(blocking)
    } else {
        GateDecision::Flag(findings)
    }
}

/// Pattern evaluation without gating policy — raw findings.
pub fn find_concerns(content: &str) -> Vec<SuperegoFinding> {
    let mut findings = Vec::new();
    let lower = content.to_lowercase();

    if !abigail_core::key_detection::detect_api_keys(content).is_empty() {
        findings.push(SuperegoFinding {
            rule: "secret-leak".to_string(),
            severity: Severity::Critical,
            description: "Content contains what looks like an API key or secret.".to_string(),
        });
    }

    if DESTRUCTIVE_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        findings.push(SuperegoFinding {
            rule: "destructive-pattern".to_string(),
            severity: Severity::Critical,
            description: "Content contains potentially destructive patterns.".to_string(),
        });
    }

    if has_pii_email(&lower) {
        findings.push(SuperegoFinding {
            rule: "pii-email".to_string(),
            severity: Severity::Warning,
            description: "Content may contain an email address.".to_string(),
        });
    }

    if has_pii_ssn(&lower) {
        findings.push(SuperegoFinding {
            rule: "pii-ssn".to_string(),
            severity: Severity::Critical,
            description: "Content may contain a social security number.".to_string(),
        });
    }

    findings
}

/// Which critical rules block in which contexts.
///
/// - Secrets and SSNs never leave the entity, regardless of context.
/// - Destructive patterns block execution surfaces (tools, jobs) but are
///   allowed in conversation — explaining a dangerous command is not running it.
fn blocks_in_context(rule: &str, context: EvaluationContext) -> bool {
    match rule {
        "secret-leak" | "pii-ssn" => true,
        "destructive-pattern" => matches!(
            context,
            EvaluationContext::ToolExecution | EvaluationContext::JobResult
        ),
        _ => false,
    }
}

fn has_pii_email(text: &str) -> bool {
    text.split_whitespace().any(|word| {
        word.contains('@') && word.contains('.') && word.len() > 5 && !word.starts_with("http")
    })
}

fn has_pii_ssn(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    chars.windows(11).any(|window| {
        window[3] == '-'
            && window[6] == '-'
            && window
                .iter()
                .enumerate()
                .all(|(i, c)| i == 3 || i == 6 || c.is_ascii_digit())
    })
}

// ── LLM judgment helpers ────────────────────────────────────────────

/// Verdict returned by the LLM judgment pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmVerdict {
    /// "clear", "concern", or "violation".
    pub verdict: String,
    #[serde(default)]
    pub reason: String,
}

/// Build the (system, user) prompts for judging an exchange against the
/// entity's constitution. The caller runs these on the local Id provider.
pub fn build_llm_judgment_prompt(
    entity_name: &str,
    constitution_excerpt: &str,
    user_message: &str,
    reply: &str,
) -> (String, String) {
    let system = format!(
        "You are the Superego of {entity_name} — its conscience. Judge whether the \
         entity's reply honors its constitution. Respond with ONLY a JSON object: \
         {{\"verdict\": \"clear\" | \"concern\" | \"violation\", \"reason\": \"<one sentence>\"}}.\n\n\
         Constitution excerpt:\n{constitution_excerpt}"
    );
    let user = format!("Mentor said:\n{user_message}\n\nEntity replied:\n{reply}");
    (system, user)
}

/// Parse an LLM judgment response, tolerating prose around the JSON object.
pub fn parse_llm_verdict(raw: &str) -> Option<LlmVerdict> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }
    let verdict: LlmVerdict = serde_json::from_str(&raw[start..=end]).ok()?;
    match verdict.verdict.as_str() {
        "clear" | "concern" | "violation" => Some(verdict),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_chat_reply_is_allowed() {
        assert!(matches!(
            evaluate(
                "The weather looks lovely today.",
                EvaluationContext::ChatReply
            ),
            GateDecision::Allow
        ));
    }

    #[test]
    fn destructive_talk_is_flagged_not_blocked_in_chat() {
        let decision = evaluate(
            "Careful: `rm -rf /` deletes everything on the disk.",
            EvaluationContext::ChatReply,
        );
        assert!(matches!(decision, GateDecision::Flag(_)));
    }

    #[test]
    fn destructive_content_blocks_tool_execution() {
        let decision = evaluate("rm -rf /home/family", EvaluationContext::ToolExecution);
        assert!(matches!(decision, GateDecision::Block(_)));
    }

    #[test]
    fn secret_leak_blocks_even_in_chat() {
        let decision = evaluate(
            "Your key is sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789",
            EvaluationContext::ChatReply,
        );
        assert!(matches!(decision, GateDecision::Block(_)));
        assert_eq!(decision.findings()[0].rule, "secret-leak");
    }

    #[test]
    fn ssn_blocks_in_any_context() {
        let decision = evaluate("ssn: 123-45-6789", EvaluationContext::ChatReply);
        assert!(matches!(decision, GateDecision::Block(_)));
    }

    #[test]
    fn email_is_a_warning_flag() {
        let decision = evaluate("write to grandma@example.com", EvaluationContext::ChatReply);
        match decision {
            GateDecision::Flag(findings) => {
                assert_eq!(findings[0].rule, "pii-email");
                assert_eq!(findings[0].severity, Severity::Warning);
            }
            other => panic!("expected flag, got {:?}", other.verdict()),
        }
    }

    #[test]
    fn llm_verdict_parses_with_surrounding_prose() {
        let raw = "Here is my judgment: {\"verdict\": \"concern\", \"reason\": \"shares location\"} hope that helps";
        let verdict = parse_llm_verdict(raw).unwrap();
        assert_eq!(verdict.verdict, "concern");
        assert_eq!(verdict.reason, "shares location");
    }

    #[test]
    fn llm_verdict_rejects_unknown_verdicts() {
        assert!(parse_llm_verdict("{\"verdict\": \"banana\"}").is_none());
        assert!(parse_llm_verdict("no json here").is_none());
    }
}
