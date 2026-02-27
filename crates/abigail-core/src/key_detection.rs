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
}
