//! CLI-based LLM provider adapter.
//!
//! Spawns an external CLI tool (Claude Code, Gemini CLI, OpenAI Codex CLI, or xAI Grok CLI)
//! as a subprocess and captures its stdout as the completion response. This lets users route
//! Ego queries through any installed CLI tool using their existing API keys.

use crate::cognitive::provider::{CompletionRequest, CompletionResponse, LlmProvider};
use abigail_core::CliPermissionMode;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::time::Duration;
use tokio::process::Command;

/// Suppress the transient console window that `Command::new()` opens on Windows.
#[cfg(windows)]
fn hide_console_window(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// Suppress the transient console window for async `tokio::process::Command` on Windows.
#[cfg(windows)]
fn hide_console_window_async(cmd: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

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

/// All known CLI variants for iteration.
pub const ALL_CLI_VARIANTS: &[CliVariant] = &[
    CliVariant::ClaudeCode,
    CliVariant::GeminiCli,
    CliVariant::OpenAiCodex,
    CliVariant::XaiGrokCli,
];

/// Result of detecting and verifying a CLI tool on the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliDetectionResult {
    pub provider_name: String,
    pub binary: String,
    pub on_path: bool,
    pub is_official: bool,
    pub is_authenticated: bool,
    pub version: Option<String>,
    /// Human-readable hint when not authenticated.
    pub auth_hint: Option<String>,
}

impl CliVariant {
    /// Expected substring in `--version` output that confirms the binary is official.
    fn official_version_marker(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::GeminiCli => "gemini",
            Self::OpenAiCodex => "codex",
            Self::XaiGrokCli => "grok",
        }
    }

    /// Auth command and expected behaviour:
    ///   - Claude / Gemini: `<binary> auth status` (exit 0 = OK, exit 1 = not authed)
    ///   - Codex / Grok:    rely on env-var presence (no `auth status` subcommand)
    fn auth_strategy(self) -> CliAuthStrategy {
        match self {
            Self::ClaudeCode => CliAuthStrategy::SubCommand("auth", "status"),
            Self::GeminiCli => CliAuthStrategy::SubCommand("auth", "status"),
            Self::OpenAiCodex => CliAuthStrategy::EnvVar("OPENAI_API_KEY"),
            Self::XaiGrokCli => CliAuthStrategy::EnvVar("GROK_API_KEY"),
        }
    }

    fn auth_hint(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Run `claude auth login` to authenticate",
            Self::GeminiCli => "Run `gemini auth login` to authenticate",
            Self::OpenAiCodex => "Set OPENAI_API_KEY or store a key in the vault",
            Self::XaiGrokCli => "Set GROK_API_KEY or store a key in the vault",
        }
    }

    /// Detect whether this CLI tool is present, official, and authenticated.
    pub fn detect(self) -> CliDetectionResult {
        let binary = self.binary_name();
        let on_path = binary_on_path(binary);

        if !on_path {
            return CliDetectionResult {
                provider_name: self.to_string(),
                binary: binary.to_string(),
                on_path: false,
                is_official: false,
                is_authenticated: false,
                version: None,
                auth_hint: None,
            };
        }

        let (is_official, version) = check_version_official(binary, self.official_version_marker());
        let is_authenticated = check_auth(binary, self.auth_strategy());

        CliDetectionResult {
            provider_name: self.to_string(),
            binary: binary.to_string(),
            on_path: true,
            is_official,
            is_authenticated,
            version,
            auth_hint: if !is_authenticated {
                Some(self.auth_hint().to_string())
            } else {
                None
            },
        }
    }
}

enum CliAuthStrategy {
    SubCommand(&'static str, &'static str),
    EnvVar(&'static str),
}

fn binary_on_path(name: &str) -> bool {
    #[cfg(windows)]
    let check = {
        let mut cmd = std::process::Command::new("where");
        cmd.arg(name)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        hide_console_window(&mut cmd);
        cmd.status()
    };
    #[cfg(not(windows))]
    let check = std::process::Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    check.map(|s| s.success()).unwrap_or(false)
}

/// Run `<binary> --version`, capture stdout, verify it contains the expected marker.
fn check_version_official(binary: &str, marker: &str) -> (bool, Option<String>) {
    let mut cmd = std::process::Command::new(binary);
    cmd.arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    hide_console_window(&mut cmd);
    let output = cmd.output();

    match output {
        Ok(o) if o.status.success() => {
            let ver_str = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let stderr_str = String::from_utf8_lossy(&o.stderr).trim().to_string();
            let combined = format!("{} {}", ver_str, stderr_str).to_lowercase();
            let is_official = combined.contains(&marker.to_lowercase());
            let version = if ver_str.is_empty() {
                None
            } else {
                Some(ver_str)
            };
            (is_official, version)
        }
        _ => (false, None),
    }
}

/// Check whether the CLI is authenticated using the variant's strategy.
fn check_auth(binary: &str, strategy: CliAuthStrategy) -> bool {
    match strategy {
        CliAuthStrategy::SubCommand(arg1, arg2) => {
            let mut cmd = std::process::Command::new(binary);
            cmd.args([arg1, arg2])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            #[cfg(windows)]
            hide_console_window(&mut cmd);
            cmd.status().map(|s| s.success()).unwrap_or(false)
        }
        CliAuthStrategy::EnvVar(var) => std::env::var(var)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .is_some(),
    }
}

/// Detect all CLI tools in a single pass.
pub fn detect_all_cli_providers() -> Vec<CliDetectionResult> {
    ALL_CLI_VARIANTS.iter().map(|v| v.detect()).collect()
}

/// An LLM provider that delegates to an external CLI tool.
///
/// For `ClaudeCode`, tracks the active session ID so subsequent messages
/// in the same conversation can use `--resume` instead of replaying all
/// history, keeping the command line short and leveraging Claude's built-in
/// session state.
pub struct CliLlmProvider {
    variant: CliVariant,
    api_key: String,
    /// Retained for API compatibility — permission flags are no longer applied
    /// per-mode; the CLI subprocess always runs with `--dangerously-skip-permissions`.
    #[allow(dead_code)]
    permission_mode: CliPermissionMode,
    /// Claude Code session ID for multi-turn continuity.
    active_session_id: RwLock<Option<String>>,
}

static CLI_PERMISSION_MODE_WARNED: AtomicBool = AtomicBool::new(false);

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
        Ok(Self {
            variant,
            api_key,
            permission_mode: CliPermissionMode::default(),
            active_session_id: RwLock::new(None),
        })
    }

    /// Create a CLI provider with a specific permission mode.
    pub fn with_permission_mode(
        variant: CliVariant,
        api_key: String,
        permission_mode: CliPermissionMode,
    ) -> anyhow::Result<Self> {
        if api_key.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "API key for {} CLI provider must not be empty",
                variant
            ));
        }
        Ok(Self {
            variant,
            api_key,
            permission_mode,
            active_session_id: RwLock::new(None),
        })
    }

    /// Clear the active session so the next request starts fresh.
    pub fn reset_session(&self) {
        if let Ok(mut guard) = self.active_session_id.write() {
            *guard = None;
        }
    }

    /// Return the current session ID, if any.
    pub fn session_id(&self) -> Option<String> {
        self.active_session_id.read().ok()?.clone()
    }

    pub fn variant(&self) -> CliVariant {
        self.variant
    }

    fn runtime_permission_posture(&self) -> &'static str {
        match self.variant {
            CliVariant::ClaudeCode => "claude:--dangerously-skip-permissions",
            CliVariant::OpenAiCodex => "codex:--full-auto",
            CliVariant::GeminiCli => "gemini:no-runtime-permission-flag",
            CliVariant::XaiGrokCli => "grok:no-runtime-permission-flag",
        }
    }

    fn warn_if_permission_mode_not_effective(&self) {
        if self.permission_mode == CliPermissionMode::DangerousSkipAll {
            return;
        }
        if !CLI_PERMISSION_MODE_WARNED.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                configured_mode = ?self.permission_mode,
                variant = %self.variant,
                runtime_posture = %self.runtime_permission_posture(),
                "cli_permission_mode is config-only in this build; runtime uses fixed variant-specific posture"
            );
        }
    }

    /// Check whether the CLI binary is available on PATH (synchronous).
    pub fn is_available(&self) -> bool {
        let mut cmd = std::process::Command::new(self.variant.binary_name());
        cmd.arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        hide_console_window(&mut cmd);
        cmd.status().map(|s| s.success()).unwrap_or(false)
    }

    /// Build a single prompt string from the non-system messages.
    pub fn build_prompt(messages: &[crate::cognitive::provider::Message]) -> String {
        let mut parts = Vec::new();
        for msg in messages {
            match msg.role.as_str() {
                "system" => {} // system messages handled separately via flags
                "assistant" => parts.push(format!("[Assistant]\n{}", msg.content)),
                _ => parts.push(msg.content.clone()),
            }
        }
        parts.join("\n\n")
    }

    /// Extract the combined system prompt from messages.
    fn extract_system_prompt(messages: &[crate::cognitive::provider::Message]) -> Option<String> {
        let parts: Vec<&str> = messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.as_str())
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
    }

    /// Add permission flags for the Claude CLI subprocess.
    ///
    /// Entity-level tool permissions are enforced by `SkillSandbox` /
    /// `SkillExecutor`, not by the CLI tool's permission system.  The CLI
    /// tool's `--allowedTools` flag expects the CLI's *own* tool names
    /// (e.g. "Bash", "Read", "Write"), not the entity's qualified skill
    /// tool names, so passing entity tool names has no effect — and on
    /// Windows the resulting command-line length overflows the `cmd.exe`
    /// 8 191-char limit (OS error 206).
    ///
    /// For Claude CLI we pass `--dangerously-skip-permissions` and rely on
    /// the entity's own layered security model.
    fn apply_permission_flags(&self, cmd: &mut Command) {
        cmd.arg("--dangerously-skip-permissions");
    }

    /// Build the full CLI command with rich flags per variant.
    ///
    /// Returns `(Command, Option<stdin_content>)`. When the second value is
    /// `Some`, the caller must pipe that string into the child's stdin to avoid
    /// Windows command-line length limits (32 767 chars for `CreateProcess`).
    fn build_command(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
    ) -> (Command, Option<String>) {
        let mut cmd = Command::new(self.variant.binary_name());

        self.warn_if_permission_mode_not_effective();

        if self.api_key != "system" {
            cmd.env(self.variant.api_key_env_var(), &self.api_key);
        }

        let stdin_content;

        match self.variant {
            CliVariant::ClaudeCode => {
                let has_session = self.active_session_id.read().ok().and_then(|g| g.clone());

                if let Some(ref sid) = has_session {
                    // Resume existing session — Claude keeps the system
                    // prompt and conversation state. Only send new input.
                    cmd.arg("--resume").arg(sid);
                    cmd.arg("--print");
                    cmd.arg("--output-format").arg("text");
                    cmd.arg("--max-turns").arg("5");
                    self.apply_permission_flags(&mut cmd);
                    cmd.stdin(std::process::Stdio::piped());
                    stdin_content = Some(prompt.to_string());
                } else {
                    // First message — send system prompt + user prompt.
                    cmd.arg("--print");
                    cmd.arg("--output-format").arg("text");
                    cmd.arg("--max-turns").arg("5");
                    self.apply_permission_flags(&mut cmd);

                    let piped = match system_prompt {
                        Some(sp) => format!(
                            "[System Instructions]\n{}\n\n[User Message]\n{}",
                            sp, prompt
                        ),
                        None => prompt.to_string(),
                    };
                    cmd.stdin(std::process::Stdio::piped());
                    stdin_content = Some(piped);
                }
            }
            CliVariant::GeminiCli => {
                cmd.arg("--prompt");
                if let Some(sp) = system_prompt {
                    let tmp = std::env::temp_dir().join("abigail_gemini_system.md");
                    let _ = std::fs::write(&tmp, sp);
                    cmd.env("GEMINI_SYSTEM_MD", tmp);
                }
                cmd.arg(prompt);
                stdin_content = None;
            }
            CliVariant::OpenAiCodex => {
                cmd.arg("exec");
                cmd.arg("--full-auto");
                let full_prompt = if let Some(sp) = system_prompt {
                    format!("[System Instructions]\n{}\n\n{}", sp, prompt)
                } else {
                    prompt.to_string()
                };
                cmd.stdin(std::process::Stdio::piped());
                stdin_content = Some(full_prompt);
            }
            CliVariant::XaiGrokCli => {
                let full_prompt = if let Some(sp) = system_prompt {
                    format!("[System Instructions]\n{}\n\n{}", sp, prompt)
                } else {
                    prompt.to_string()
                };
                cmd.stdin(std::process::Stdio::piped());
                stdin_content = Some(full_prompt);
            }
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        hide_console_window_async(&mut cmd);
        (cmd, stdin_content)
    }

    /// Spawn the CLI process and wait for completion with timeout.
    ///
    /// When `stdin_content` is `Some`, the string is written to the child's
    /// stdin before waiting — this avoids the Windows command-line length limit.
    async fn run_and_collect(
        &self,
        mut cmd: Command,
        timeout_secs: u64,
        stdin_content: Option<String>,
    ) -> anyhow::Result<String> {
        let mut child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "Failed to spawn {} CLI (is '{}' on PATH?): {}",
                self.variant,
                self.variant.binary_name(),
                e
            )
        })?;

        if let Some(content) = stdin_content {
            use tokio::io::AsyncWriteExt;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(content.as_bytes()).await?;
                drop(stdin);
            }
        }

        let output =
            tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output())
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "{} CLI timed out after {} seconds",
                        self.variant,
                        timeout_secs
                    )
                })??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "{} CLI exited with {}: {}",
                self.variant,
                output.status,
                stderr.trim()
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

#[async_trait]
impl LlmProvider for CliLlmProvider {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        if let Some(ref override_model) = request.model_override {
            tracing::warn!(
                "CliLlmProvider ignoring model_override='{}' — CLI variant {} uses its own model selection",
                override_model,
                self.variant,
            );
        }

        let system_prompt = Self::extract_system_prompt(&request.messages);
        let prompt = Self::build_prompt(&request.messages);

        tracing::info!(
            "CliLlmProvider::complete variant={}, binary={}, prompt_len={}, has_system_prompt={}, has_tools={}, configured_permission_mode={:?}, runtime_permission_posture={}",
            self.variant,
            self.variant.binary_name(),
            prompt.len(),
            system_prompt.is_some(),
            request.tools.is_some(),
            self.permission_mode,
            self.runtime_permission_posture(),
        );

        let (cmd, stdin_content) = self.build_command(&prompt, system_prompt.as_deref());

        tracing::info!(
            "CLI stdin payload: system_prompt={} bytes, user_prompt={} bytes, total_stdin={} bytes",
            system_prompt.as_ref().map(|s| s.len()).unwrap_or(0),
            prompt.len(),
            stdin_content.as_ref().map(|s| s.len()).unwrap_or(0),
        );

        let content = self.run_and_collect(cmd, 300, stdin_content).await?;

        tracing::info!(
            "CLI subprocess completed. Output size: {} bytes",
            content.len()
        );

        Ok(CompletionResponse {
            content,
            tool_calls: None,
        })
    }

    async fn stream(
        &self,
        request: &CompletionRequest,
        tx: tokio::sync::mpsc::Sender<crate::cognitive::provider::StreamEvent>,
    ) -> anyhow::Result<CompletionResponse> {
        use crate::cognitive::provider::StreamEvent;
        use tokio::io::AsyncBufReadExt;

        if self.variant != CliVariant::ClaudeCode {
            return self.complete(request).await.inspect(|resp| {
                let _ = tx.try_send(StreamEvent::Token(resp.content.clone()));
                let _ = tx.try_send(StreamEvent::Done(resp.clone()));
            });
        }

        let system_prompt = Self::extract_system_prompt(&request.messages);
        let prompt = Self::build_prompt(&request.messages);

        let has_session = self.active_session_id.read().ok().and_then(|g| g.clone());

        let mut cmd = Command::new(self.variant.binary_name());
        self.warn_if_permission_mode_not_effective();
        if self.api_key != "system" {
            cmd.env(self.variant.api_key_env_var(), &self.api_key);
        }

        let piped: String;
        if let Some(ref sid) = has_session {
            cmd.arg("--resume").arg(sid);
            cmd.arg("--print");
            cmd.arg("--output-format").arg("stream-json");
            cmd.arg("--max-turns").arg("5");
            self.apply_permission_flags(&mut cmd);
            piped = prompt.clone();
        } else {
            cmd.arg("--print");
            cmd.arg("--output-format").arg("stream-json");
            cmd.arg("--max-turns").arg("5");
            self.apply_permission_flags(&mut cmd);
            piped = match system_prompt.as_deref() {
                Some(sp) => format!(
                    "[System Instructions]\n{}\n\n[User Message]\n{}",
                    sp, prompt
                ),
                None => prompt.clone(),
            };
        }

        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        hide_console_window_async(&mut cmd);

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn claude CLI for streaming: {}", e))?;

        {
            use tokio::io::AsyncWriteExt;
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(piped.as_bytes()).await;
                drop(stdin);
            }
        }

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stdout from claude CLI"))?;

        let mut reader = tokio::io::BufReader::new(stdout).lines();
        let mut full_content = String::new();
        let mut captured_session_id: Option<String> = None;

        while let Ok(Some(line)) = reader.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
                // Capture session ID from Claude's stream output for reuse.
                if captured_session_id.is_none() {
                    if let Some(sid) = event
                        .get("session_id")
                        .or_else(|| event.get("sessionId"))
                        .and_then(|v| v.as_str())
                    {
                        captured_session_id = Some(sid.to_string());
                    }
                }

                if let Some(delta) = event
                    .get("content_block_delta")
                    .or_else(|| event.get("delta"))
                    .and_then(|d| d.get("text"))
                    .and_then(|t| t.as_str())
                {
                    full_content.push_str(delta);
                    let _ = tx.send(StreamEvent::Token(delta.to_string())).await;
                } else if let Some(result) = event.get("result").and_then(|r| r.as_str()) {
                    if full_content.is_empty() {
                        full_content = result.to_string();
                    }
                }
            }
        }

        let _ = child.wait().await;

        // Store session ID for subsequent --resume calls.
        if let Some(sid) = captured_session_id {
            if let Ok(mut guard) = self.active_session_id.write() {
                *guard = Some(sid);
            }
        }

        let response = CompletionResponse {
            content: full_content,
            tool_calls: None,
        };
        let _ = tx.send(StreamEvent::Done(response.clone())).await;
        Ok(response)
    }
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
        // System messages are now handled via CLI flags (--append-system-prompt),
        // so build_prompt excludes them from the prompt string.
        let prompt = CliLlmProvider::build_prompt(&messages);
        assert_eq!(prompt, "Hello");
    }

    #[test]
    fn test_extract_system_prompt() {
        let messages = vec![
            Message::new("system", "You are helpful."),
            Message::new("user", "Hello"),
        ];
        let sys = CliLlmProvider::extract_system_prompt(&messages);
        assert_eq!(sys, Some("You are helpful.".to_string()));
    }

    #[test]
    fn test_extract_system_prompt_none() {
        let messages = vec![Message::new("user", "Hello")];
        let sys = CliLlmProvider::extract_system_prompt(&messages);
        assert!(sys.is_none());
    }
}
