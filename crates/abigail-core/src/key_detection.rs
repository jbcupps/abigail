//! Pure API key detection from freeform text.
//!
//! Extracted from the Tauri app so that both the desktop GUI, CLI, and daemon
//! pipelines can reuse the same detection logic without depending on `AppState`.

/// Regex patterns for API key detection. Each entry: (pattern, provider_name).
pub const KEY_PATTERNS: &[(&str, &str)] = &[
    (r"sk-ant-[a-zA-Z0-9_-]{20,}", "anthropic"),
    (r"sk-[a-zA-Z0-9]{20,}", "openai"),
    (r"xai-[a-zA-Z0-9_-]{20,}", "xai"),
    (r"pplx-[a-zA-Z0-9_-]{20,}", "perplexity"),
    (r"AIza[a-zA-Z0-9_-]{35}", "google"),
    (r"tvly-[a-zA-Z0-9_-]{20,}", "tavily"),
];

/// Alias mapping: when a key is detected for a provider, also store it under
/// these names so that CLI-based providers are automatically configured.
pub const CLI_ALIASES: &[(&str, &str)] = &[
    ("openai", "codex-cli"),
    ("anthropic", "claude-cli"),
    ("google", "gemini-cli"),
    ("xai", "grok-cli"),
];

/// Scan a message for API key patterns.
/// Returns a vec of `(provider_name, key_string)` tuples.
pub fn detect_api_keys(message: &str) -> Vec<(String, String)> {
    let mut detected = Vec::new();
    for (pattern, provider) in KEY_PATTERNS {
        if let Ok(re) = regex::Regex::new(pattern) {
            for mat in re.find_iter(message) {
                detected.push((provider.to_string(), mat.as_str().to_string()));
            }
        }
    }
    detected
}

/// Redact detected API key patterns in text, replacing the sensitive portion
/// with a short visible prefix and `***`. This is the backend-side equivalent
/// of the UI `redactApiKeys` function.
pub fn redact_secrets(text: &str) -> String {
    let mut result = text.to_string();
    for (pattern, _provider) in KEY_PATTERNS {
        if let Ok(re) = regex::Regex::new(pattern) {
            result = re
                .replace_all(&result, |caps: &regex::Captures<'_>| {
                    let matched = caps.get(0).map(|m| m.as_str()).unwrap_or("");
                    let dash_idx = matched.find('-');
                    let visible = if let Some(idx) = dash_idx {
                        &matched[..std::cmp::min(idx + 4, matched.len())]
                    } else {
                        &matched[..std::cmp::min(4, matched.len())]
                    };
                    format!("{}***", visible)
                })
                .to_string();
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_openai_key() {
        let msg = "Here is my key: sk-abcdefghijklmnopqrstuvwxyz";
        let keys = detect_api_keys(msg);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].0, "openai");
    }

    #[test]
    fn detects_anthropic_key() {
        let msg = "Use sk-ant-abc123def456ghi789jklmno";
        let keys = detect_api_keys(msg);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].0, "anthropic");
    }

    #[test]
    fn detects_google_key() {
        let msg = "AIzaSyAbCdEfGhIjKlMnOpQrStUvWxYz12345678901";
        let keys = detect_api_keys(msg);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].0, "google");
    }

    #[test]
    fn detects_xai_key() {
        let msg = "xai-abcdefghijklmnopqrstuvwxyz";
        let keys = detect_api_keys(msg);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].0, "xai");
    }

    #[test]
    fn no_false_positives_on_normal_message() {
        let msg = "Hello, how are you? I want to build a project with React and Rust.";
        let keys = detect_api_keys(msg);
        assert!(keys.is_empty());
    }

    #[test]
    fn detects_multiple_keys() {
        let msg =
            "OpenAI: sk-abcdefghijklmnopqrstuvwxyz and Anthropic: sk-ant-abc123def456ghi789jklmno";
        let keys = detect_api_keys(msg);
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn redact_openai_key() {
        let text = "key is sk-abcdefghijklmnopqrstuvwxyz ok";
        let redacted = redact_secrets(text);
        assert!(redacted.contains("sk-abc***"));
        assert!(!redacted.contains("abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn redact_anthropic_key() {
        let text = "use sk-ant-abc123def456ghi789jklmno for auth";
        let redacted = redact_secrets(text);
        assert!(
            redacted.contains("***"),
            "should contain redaction marker, got: {}",
            redacted
        );
        assert!(
            !redacted.contains("abc123def456"),
            "should not contain raw key body"
        );
    }

    #[test]
    fn redact_preserves_normal_text() {
        let text = "Hello, this is a normal message with no secrets.";
        assert_eq!(redact_secrets(text), text);
    }

    #[test]
    fn redact_multiple_keys() {
        let text = "openai=sk-abcdefghijklmnopqrstuvwxyz anthropic=sk-ant-abc123def456ghi789jklmno";
        let redacted = redact_secrets(text);
        assert!(!redacted.contains("abcdefghijklmnopqrstuvwxyz"));
        assert!(!redacted.contains("abc123def456"));
    }
}
