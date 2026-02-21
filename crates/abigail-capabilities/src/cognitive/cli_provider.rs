//! CLI-based LLM provider adapter.
//!
//! Spawns an external CLI tool (Claude Code, Gemini CLI, OpenAI Codex CLI, or xAI Grok CLI)
//! as a subprocess and captures its stdout as the completion response. This lets users route
//! Ego queries through any installed CLI tool using their existing API keys.

use crate::cognitive::provider::{CompletionRequest, CompletionResponse, LlmProvider};
use async_trait::async_trait;
use std::time::Duration;
use tokio::process::Command;

/// Which CLI tool to invoke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliVariant {
    /// `claude --print "<prompt>"`  — env: ANTHROPIC_API_KEY
    ClaudeCode,
    /// `gemini "<prompt>"`  — env: GOOGLE_API_KEY
    GeminiCli,
    /// `codex --quiet "<prompt>"`  — env: OPENAI_API_KEY
    OpenAiCodex,
    /// `grok "<prompt>"`  — env: XAI_API_KEY
    XaiGrokCli,
}

impl CliVariant {
    /// The executable name to spawn.
    pub fn binary_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::GeminiCli => "gemini",
            Self::OpenAiCodex => "codex",
            Self::XaiGrokCli => "grok",
        }
    }

    /// The environment variable used to pass the API key to the subprocess.
    pub fn api_key_env_var(self) -> &'static str {
        match self {
            Self::ClaudeCode => "ANTHROPIC_API_KEY",
            Self::GeminiCli => "GOOGLE_API_KEY",
            Self::OpenAiCodex => "OPENAI_API_KEY",
            Self::XaiGrokCli => "XAI_API_KEY",
        }
    }

    /// Parse a variant from a name string (case-insensitive).
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "claude-code" | "claude-cli" | "claude_code" | "claude_cli" => Some(Self::ClaudeCode),
            "gemini-cli" | "gemini_cli" => Some(Self::GeminiCli),
            "codex-cli" | "codex_cli" | "openai-codex" | "openai_codex" => Some(Self::OpenAiCodex),
            "grok-cli" | "grok_cli" | "xai-grok" | "xai_grok" => Some(Self::XaiGrokCli),
            _ => None,
        }
    }
}

impl std::fmt::Display for CliVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClaudeCode => write!(f, "claude-cli"),
            Self::GeminiCli => write!(f, "gemini-cli"),
            Self::OpenAiCodex => write!(f, "codex-cli"),
            Self::XaiGrokCli => write!(f, "grok-cli"),
        }
    }
}

/// An LLM provider that delegates to an external CLI tool.
pub struct CliLlmProvider {
    variant: CliVariant,
    api_key: String,
}

impl CliLlmProvider {
    /// Create a new CLI provider. Returns an error if the API key is empty.
    /// Use "system" as the key to rely on the CLI's internal auth (e.g. OAuth).
    pub fn new(variant: CliVariant, api_key: String) -> anyhow::Result<Self> {
        if api_key.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "API key for {} CLI provider must not be empty",
                variant
            ));
        }
        Ok(Self { variant, api_key })
    }

    /// Check whether the CLI binary is available on PATH (synchronous).
    pub fn is_available(&self) -> bool {
        std::process::Command::new(self.variant.binary_name())
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Build a single prompt string from the message list.
    pub fn build_prompt(messages: &[crate::cognitive::provider::Message]) -> String {
        let mut parts = Vec::new();
        for msg in messages {
            match msg.role.as_str() {
                "system" => parts.push(format!("[System]\n{}", msg.content)),
                "assistant" => parts.push(format!("[Assistant]\n{}", msg.content)),
                _ => parts.push(msg.content.clone()),
            }
        }
        parts.join("\n\n")
    }

    /// Configure the CLI command with variant-specific flags.
    fn configure_command(&self, cmd: &mut Command, prompt: &str) {
        match self.variant {
            CliVariant::ClaudeCode => {
                cmd.arg("--print").arg(prompt);
            }
            CliVariant::GeminiCli => {
                cmd.arg(prompt);
            }
            CliVariant::OpenAiCodex => {
                cmd.arg("--quiet").arg(prompt);
            }
            CliVariant::XaiGrokCli => {
                cmd.arg(prompt);
            }
        }
    }
}

#[async_trait]
impl LlmProvider for CliLlmProvider {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let prompt = Self::build_prompt(&request.messages);

        tracing::info!(
            "CliLlmProvider::complete variant={}, prompt_len={}",
            self.variant,
            prompt.len(),
        );

        let mut cmd = Command::new(self.variant.binary_name());
        if self.api_key != "system" {
            cmd.env(self.variant.api_key_env_var(), &self.api_key);
        }
        self.configure_command(&mut cmd, &prompt);

        // Capture stdout, discard stderr
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "Failed to spawn {} CLI (is '{}' on PATH?): {}",
                self.variant,
                self.variant.binary_name(),
                e
            )
        })?;

        let output = tokio::time::timeout(Duration::from_secs(120), child.wait_with_output())
            .await
            .map_err(|_| anyhow::anyhow!("{} CLI timed out after 120 seconds", self.variant))??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "{} CLI exited with {}: {}",
                self.variant,
                output.status,
                stderr.trim()
            ));
        }

        let content = String::from_utf8_lossy(&output.stdout).trim().to_string();

        Ok(CompletionResponse {
            content,
            tool_calls: None,
        })
    }

    // stream() uses the default trait fallback (complete → single Token event).
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cognitive::provider::Message;

    #[test]
    fn test_cli_variant_from_name() {
        assert_eq!(
            CliVariant::from_name("claude-cli"),
            Some(CliVariant::ClaudeCode)
        );
        assert_eq!(
            CliVariant::from_name("claude-code"),
            Some(CliVariant::ClaudeCode)
        );
        assert_eq!(
            CliVariant::from_name("gemini-cli"),
            Some(CliVariant::GeminiCli)
        );
        assert_eq!(
            CliVariant::from_name("codex-cli"),
            Some(CliVariant::OpenAiCodex)
        );
        assert_eq!(
            CliVariant::from_name("openai-codex"),
            Some(CliVariant::OpenAiCodex)
        );
        assert_eq!(
            CliVariant::from_name("grok-cli"),
            Some(CliVariant::XaiGrokCli)
        );
        assert_eq!(
            CliVariant::from_name("xai-grok"),
            Some(CliVariant::XaiGrokCli)
        );
        assert_eq!(CliVariant::from_name("unknown"), None);
    }

    #[test]
    fn test_cli_variant_display() {
        assert_eq!(CliVariant::ClaudeCode.to_string(), "claude-cli");
        assert_eq!(CliVariant::GeminiCli.to_string(), "gemini-cli");
        assert_eq!(CliVariant::OpenAiCodex.to_string(), "codex-cli");
        assert_eq!(CliVariant::XaiGrokCli.to_string(), "grok-cli");
    }

    #[test]
    fn test_cli_variant_binary_names() {
        assert_eq!(CliVariant::ClaudeCode.binary_name(), "claude");
        assert_eq!(CliVariant::GeminiCli.binary_name(), "gemini");
        assert_eq!(CliVariant::OpenAiCodex.binary_name(), "codex");
        assert_eq!(CliVariant::XaiGrokCli.binary_name(), "grok");
    }

    #[test]
    fn test_cli_variant_env_vars() {
        assert_eq!(
            CliVariant::ClaudeCode.api_key_env_var(),
            "ANTHROPIC_API_KEY"
        );
        assert_eq!(CliVariant::GeminiCli.api_key_env_var(), "GOOGLE_API_KEY");
        assert_eq!(CliVariant::OpenAiCodex.api_key_env_var(), "OPENAI_API_KEY");
        assert_eq!(CliVariant::XaiGrokCli.api_key_env_var(), "XAI_API_KEY");
    }

    #[test]
    fn test_rejects_empty_api_key() {
        assert!(CliLlmProvider::new(CliVariant::ClaudeCode, String::new()).is_err());
        assert!(CliLlmProvider::new(CliVariant::GeminiCli, "   ".to_string()).is_err());
    }

    #[test]
    fn test_accepts_valid_api_key() {
        assert!(CliLlmProvider::new(CliVariant::ClaudeCode, "sk-ant-test123".to_string()).is_ok());
        assert!(CliLlmProvider::new(CliVariant::GeminiCli, "AIza-test".to_string()).is_ok());
    }

    #[test]
    fn test_build_prompt_simple() {
        let messages = vec![Message::new("user", "What is Rust?")];
        let prompt = CliLlmProvider::build_prompt(&messages);
        assert_eq!(prompt, "What is Rust?");
    }

    #[test]
    fn test_build_prompt_with_system() {
        let messages = vec![
            Message::new("system", "You are helpful."),
            Message::new("user", "Hello"),
        ];
        let prompt = CliLlmProvider::build_prompt(&messages);
        assert_eq!(prompt, "[System]\nYou are helpful.\n\nHello");
    }
}
