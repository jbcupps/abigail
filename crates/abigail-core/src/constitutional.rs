//! Constitutional context helpers for monitor-time preprompt enrichment.
//!
//! Loads constitutional text from `templates/` when available and falls back
//! to compiled defaults. Returns a compact context block for mentor monitor use.

use crate::templates;
use std::path::{Path, PathBuf};

const MAX_SNIPPET_LEN: usize = 380;

/// Build a compact constitutional preprompt context for mentor monitor injection.
///
/// Reads `templates/soul.md`, `templates/ethics.md`, and `templates/instincts.md`
/// when present in the current working tree. If files are not present, compiled-in
/// defaults are used.
pub async fn load_preprompt_context(user_message: &str) -> anyhow::Result<String> {
    let soul = read_template_or_fallback("soul.md", templates::SOUL_MD);
    let ethics = read_template_or_fallback("ethics.md", templates::ETHICS_MD);
    let instincts = read_template_or_fallback("instincts.md", templates::INSTINCTS_MD);

    let soul_excerpt = first_paragraph_after_title(&soul);
    let ethics_excerpt = first_n_bullets(&ethics, 3);
    let instincts_excerpt = first_n_bullets(&instincts, 3);

    let lower = user_message.to_lowercase();
    let id_context = infer_id_context(&lower);
    let superego_context = infer_superego_context(&lower);

    Ok(format!(
        "## Constitutional Monitor Context\n\
         Soul: {soul}\n\
         Ethics anchors: {ethics}\n\
         Instinct anchors: {instincts}\n\n\
         ## Runtime Signals\n\
         - Id context: {id_context}\n\
         - Superego context: {superego_context}\n\
         - Out-of-band observers (memory/id/superego) are passive and non-blocking.",
        soul = clamp(&soul_excerpt, MAX_SNIPPET_LEN),
        ethics = clamp(&ethics_excerpt, MAX_SNIPPET_LEN),
        instincts = clamp(&instincts_excerpt, MAX_SNIPPET_LEN),
        id_context = id_context,
        superego_context = superego_context,
    ))
}

pub fn infer_id_context(lower_message: &str) -> &'static str {
    if contains_any(
        lower_message,
        &[
            "password",
            "token",
            "secret",
            "ssn",
            "credit card",
            "private key",
            "api key",
        ],
    ) {
        "privacy-sensitive: keep processing local-first, redact before any external routing"
    } else if contains_any(
        lower_message,
        &[
            "search", "web", "latest", "news", "http://", "https://", "api",
        ],
    ) {
        "external-context likely required: route with explicit source-check behavior"
    } else {
        "routine request profile: low-latency local path remains acceptable fallback"
    }
}

pub fn infer_superego_context(lower_message: &str) -> &'static str {
    if contains_any(
        lower_message,
        &[
            "rm -rf",
            "drop table",
            "delete all",
            "wipe",
            "disable safety",
            "bypass policy",
        ],
    ) {
        "high-risk pattern detected: block destructive actions pending mentor approval"
    } else if contains_any(lower_message, &["exploit", "malware", "phish", "steal"]) {
        "security-sensitive intent detected: enforce strict refusal and safe alternatives"
    } else {
        "within nominal safety posture: monitor only, no pre-emptive block"
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

fn read_template_or_fallback(filename: &str, fallback: &str) -> String {
    template_candidate_paths(filename)
        .into_iter()
        .find_map(|p| std::fs::read_to_string(&p).ok())
        .unwrap_or_else(|| fallback.to_string())
}

fn template_candidate_paths(filename: &str) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(3);
    paths.push(Path::new("templates").join(filename));

    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("templates").join(filename));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            paths.push(parent.join("templates").join(filename));
        }
    }
    paths
}

fn first_paragraph_after_title(content: &str) -> String {
    let mut started = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            started = true;
            continue;
        }
        if !started {
            continue;
        }
        if trimmed.is_empty() {
            if !lines.is_empty() {
                break;
            }
            continue;
        }
        if trimmed.starts_with("## ") && !lines.is_empty() {
            break;
        }
        lines.push(trimmed.to_string());
    }

    if lines.is_empty() {
        content.lines().take(2).collect::<Vec<_>>().join(" ")
    } else {
        lines.join(" ")
    }
}

fn first_n_bullets(content: &str, n: usize) -> String {
    let bullets: Vec<String> = content
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("- "))
        .map(|line| line.trim_start_matches("- ").trim().to_string())
        .take(n)
        .collect();

    if bullets.is_empty() {
        content.lines().take(2).collect::<Vec<_>>().join(" ")
    } else {
        bullets.join(" | ")
    }
}

fn clamp(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    let mut out = text.chars().take(max_len).collect::<String>();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn preprompt_context_contains_monitor_sections() {
        let out = load_preprompt_context("search latest provider docs")
            .await
            .unwrap();
        assert!(out.contains("Constitutional Monitor Context"));
        assert!(out.contains("Runtime Signals"));
        assert!(out.contains("Id context"));
        assert!(out.contains("Superego context"));
    }

    #[test]
    fn id_context_detects_privacy_signals() {
        let got = infer_id_context("contains api key and password");
        assert!(got.contains("privacy-sensitive"));
    }

    #[test]
    fn superego_context_detects_destructive_signals() {
        let got = infer_superego_context("please run rm -rf now");
        assert!(got.contains("high-risk pattern"));
    }
}
