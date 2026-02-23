//! Chat pipeline utilities for entity-daemon.
//!
//! Ported from `tauri-app/src/commands/chat.rs` to bring entity-daemon chat
//! to functional parity: sanitization, system prompt, tool awareness, dedup.

use abigail_capabilities::cognitive::Message;
use abigail_skills::SkillRegistry;
use entity_core::SessionMessage;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_HISTORY_MESSAGES: usize = 24;
const MAX_MESSAGE_CHARS: usize = 4_000;

// ---------------------------------------------------------------------------
// Sanitize session history
// ---------------------------------------------------------------------------

/// Filter invalid roles, trim content, cap at 24 messages / 4000 chars each.
pub fn sanitize_session_history(history: Option<Vec<SessionMessage>>) -> Vec<Message> {
    history
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| {
            if m.role != "user" && m.role != "assistant" {
                return None;
            }
            let trimmed = m.content.trim();
            if trimmed.is_empty() {
                return None;
            }
            let content = if trimmed.chars().count() > MAX_MESSAGE_CHARS {
                trimmed.chars().take(MAX_MESSAGE_CHARS).collect::<String>()
            } else {
                trimmed.to_string()
            };
            Some(Message::new(&m.role, &content))
        })
        .rev()
        .take(MAX_HISTORY_MESSAGES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

// ---------------------------------------------------------------------------
// Build contextual messages
// ---------------------------------------------------------------------------

/// Assemble `[system_prompt, ...sanitized_history, user_message]` with
/// last-message deduplication (if the final history message is identical to
/// the new user message, drop it to avoid repeating).
pub fn build_contextual_messages(
    system_prompt: &str,
    session_messages: Option<Vec<SessionMessage>>,
    latest_user_message: &str,
) -> Vec<Message> {
    let mut messages = vec![Message::new("system", system_prompt)];
    let mut history = sanitize_session_history(session_messages);

    // Deduplicate: if the last history message is the same as what the user
    // just sent, drop it so we don't feed the LLM a duplicate.
    if let Some(last) = history.last() {
        if last.role == "user" && last.content == latest_user_message.trim() {
            history.pop();
        }
    }

    messages.extend(history);
    messages.push(Message::new("user", latest_user_message));
    messages
}

// ---------------------------------------------------------------------------
// Tool awareness section
// ---------------------------------------------------------------------------

/// Build a Markdown "Available Tools" section from the skill registry.
/// Entity-daemon version — no BrowserCapability/HttpClientCapability params.
pub fn build_tool_awareness_section(registry: &SkillRegistry) -> String {
    let mut sections = Vec::new();

    if let Ok(manifests) = registry.list() {
        for manifest in &manifests {
            if let Ok((skill, _)) = registry.get_skill(&manifest.id) {
                let tools = skill.tools();
                if tools.is_empty() {
                    continue;
                }
                let mut s = format!("### {} ({})\n", manifest.name, manifest.id.0);
                for t in &tools {
                    s.push_str(&format!(
                        "- **{}::{}**: {}\n",
                        manifest.id.0, t.name, t.description
                    ));
                }
                sections.push(s);
            }
        }
    }

    if sections.is_empty() {
        String::new()
    } else {
        format!("\n\n## Available Tools\n\n{}", sections.join("\n"))
    }
}

// ---------------------------------------------------------------------------
// Risk clarification
// ---------------------------------------------------------------------------

/// Quick keyword check for risky messages that need clarification.
pub fn needs_risk_clarification(message: &str) -> bool {
    let m = message.to_lowercase();
    let risky = ["hack", "exploit", "bypass", "weapon", "malware", "ddos"];
    let has_risky = risky.iter().any(|k| m.contains(k));
    let has_safe_context =
        m.contains("defensive") || m.contains("authorized") || m.contains("training");
    has_risky && !has_safe_context
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_empty_history() {
        let result = sanitize_session_history(None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_sanitize_filters_invalid_roles() {
        let history = vec![
            SessionMessage {
                role: "user".into(),
                content: "hello".into(),
            },
            SessionMessage {
                role: "system".into(),
                content: "should be filtered".into(),
            },
            SessionMessage {
                role: "assistant".into(),
                content: "world".into(),
            },
        ];
        let result = sanitize_session_history(Some(history));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, "user");
        assert_eq!(result[1].role, "assistant");
    }

    #[test]
    fn test_sanitize_filters_empty_content() {
        let history = vec![
            SessionMessage {
                role: "user".into(),
                content: "   ".into(),
            },
            SessionMessage {
                role: "assistant".into(),
                content: "ok".into(),
            },
        ];
        let result = sanitize_session_history(Some(history));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "ok");
    }

    #[test]
    fn test_sanitize_caps_message_length() {
        let long_content = "a".repeat(5000);
        let history = vec![SessionMessage {
            role: "user".into(),
            content: long_content,
        }];
        let result = sanitize_session_history(Some(history));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content.len(), MAX_MESSAGE_CHARS);
    }

    #[test]
    fn test_sanitize_caps_history_count() {
        let history: Vec<SessionMessage> = (0..30)
            .map(|i| SessionMessage {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                content: format!("msg {}", i),
            })
            .collect();
        let result = sanitize_session_history(Some(history));
        assert_eq!(result.len(), MAX_HISTORY_MESSAGES);
        // Should keep the most recent 24 (indices 6..30)
        assert_eq!(result[0].content, "msg 6");
    }

    #[test]
    fn test_build_contextual_deduplicates_last() {
        let history = vec![
            SessionMessage {
                role: "user".into(),
                content: "hello".into(),
            },
            SessionMessage {
                role: "assistant".into(),
                content: "hi".into(),
            },
            SessionMessage {
                role: "user".into(),
                content: "how are you".into(),
            },
        ];
        let msgs = build_contextual_messages("sys", Some(history), "how are you");
        // system + user("hello") + assistant("hi") + user("how are you")
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[3].role, "user");
        assert_eq!(msgs[3].content, "how are you");
    }

    #[test]
    fn test_needs_risk_clarification_basic() {
        assert!(needs_risk_clarification("how to hack a server"));
        assert!(!needs_risk_clarification("how to bake a cake"));
        assert!(!needs_risk_clarification(
            "defensive hack detection strategies"
        ));
    }
}
