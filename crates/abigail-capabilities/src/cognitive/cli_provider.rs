//! CLI-based LLM provider adapter.
//!
//! Spawns an external CLI tool (Claude Code, Gemini CLI, OpenAI Codex CLI, or xAI Grok CLI)
//! as a subprocess and captures its stdout as the completion response. This lets users route
//! Ego queries through supported installed tools using their existing sign-in.

use crate::cognitive::provider::{CompletionRequest, CompletionResponse, LlmProvider};
use crate::cognitive::validation::is_model_compatible_with_provider;
use abigail_core::CliPermissionMode;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
    /// Codex app-server, using its own ChatGPT sign-in.
    OpenAiCodex,
    /// Grok Build ACP, using its own sign-in or XAI_API_KEY.
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

    fn model_override_provider(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-cli",
            Self::GeminiCli => "gemini-cli",
            Self::OpenAiCodex => "codex-cli",
            Self::XaiGrokCli => "grok-cli",
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
            Self::OpenAiCodex => "codex-cli",
            Self::XaiGrokCli => "grok",
        }
    }

    /// Passive discovery never makes an inference request or starts a login.
    fn auth_strategy(self) -> CliAuthStrategy {
        match self {
            Self::ClaudeCode => CliAuthStrategy::SubCommand("auth", "status"),
            Self::GeminiCli => CliAuthStrategy::SubCommand("auth", "status"),
            Self::OpenAiCodex => CliAuthStrategy::SubCommand("login", "status"),
            // Grok does not expose a passive login-status command. Its existing
            // account is checked through ACP when the user selects it in Hive.
            Self::XaiGrokCli => CliAuthStrategy::Unknown,
        }
    }

    fn auth_hint(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Run `claude auth login` to authenticate",
            Self::GeminiCli => "Run `gemini auth login` to authenticate",
            Self::OpenAiCodex => "Sign in with `codex login`, then check the connection in Hive",
            Self::XaiGrokCli => {
                "Check your existing connection in Hive. If needed, sign in with `grok login`"
            }
        }
    }

    /// Detect whether this CLI tool is present, official, and authenticated.
    pub fn detect(self) -> CliDetectionResult {
        let binary = self.binary_name();
        let resolved = resolve_cli_binary(self);
        let on_path = resolved.is_ok();

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

        let resolved = resolved.expect("resolved binary checked above");
        let (mut is_official, version) =
            check_version_official(&resolved, self.official_version_marker());
        if self == Self::XaiGrokCli && is_official {
            // Official Grok Build prints only `grok <version>` for --version.
            // Its passive help header distinguishes it from unrelated Grok CLIs.
            is_official = metadata_output(&resolved, &["--help"]).is_some_and(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout)
                        .to_ascii_lowercase()
                        .contains("grok build")
            });
        }
        let is_authenticated = check_auth(&resolved, self.auth_strategy());

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
    Unknown,
}

/// Resolve a native executable, without executing npm's shell wrappers. Windows
/// desktop launches may inherit an older PATH, so include known per-user installs.
pub(crate) fn resolve_cli_binary(variant: CliVariant) -> anyhow::Result<PathBuf> {
    let mut directories: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .filter(|entry| entry.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    #[cfg(windows)]
    {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            let profile = PathBuf::from(profile);
            directories.extend([
                profile.join(".local/bin"),
                profile.join(".grok/bin"),
                profile.join(".cargo/bin"),
            ]);
        }
        if let Some(appdata) = std::env::var_os("APPDATA") {
            directories.push(PathBuf::from(appdata).join("npm"));
        }
    }
    resolve_from_directories(variant, &directories).ok_or_else(|| anyhow::anyhow!(
        "{} is not installed in a supported location. Install its official CLI, then reopen Abigail.", variant
    ))
}

fn resolve_from_directories(variant: CliVariant, directories: &[PathBuf]) -> Option<PathBuf> {
    for directory in directories {
        let direct = directory.join(if cfg!(windows) {
            format!("{}.exe", variant.binary_name())
        } else {
            variant.binary_name().to_string()
        });
        if direct.is_file() {
            return Some(direct);
        }
        #[cfg(windows)]
        if variant == CliVariant::OpenAiCodex {
            let target = if cfg!(target_arch = "aarch64") {
                "aarch64-pc-windows-msvc"
            } else {
                "x86_64-pc-windows-msvc"
            };
            let package = if cfg!(target_arch = "aarch64") {
                "codex-win32-arm64"
            } else {
                "codex-win32-x64"
            };
            let base = directory.join("node_modules/@openai/codex");
            for candidate in [
                base.join(format!(
                    "node_modules/@openai/{package}/vendor/{target}/bin/codex.exe"
                )),
                base.join(format!("vendor/{target}/codex/codex.exe")),
                directory.join(format!(
                    "node_modules/@openai/{package}/vendor/{target}/bin/codex.exe"
                )),
            ] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Collect small metadata commands with a deadline; a hung installed tool must
/// not block Hive's first-run screen. stdout/stderr drain on a worker thread.
fn metadata_output(binary: &Path, args: &[&str]) -> Option<std::process::Output> {
    let mut cmd = std::process::Command::new(binary);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    hide_console_window(&mut cmd);
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let stderr = child.stderr.take()?;
    let collect = |mut input: Box<dyn std::io::Read + Send>| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            // Read past the retained cap so a verbose child cannot fill its pipe.
            let mut buffer = [0u8; 4096];
            while let Ok(count) = input.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                if bytes.len() < 65536 {
                    bytes.extend_from_slice(&buffer[..count.min(65536 - bytes.len())]);
                }
            }
            let _ = tx.send(bytes);
        });
        rx
    };
    let stdout = collect(Box::new(stdout));
    let stderr = collect(Box::new(stderr));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    }?;
    let stdout = stdout
        .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
        .ok()?;
    let stderr = stderr
        .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
        .ok()?;
    Some(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

/// Run `<binary> --version`, capture stdout, verify it contains the expected marker.
fn check_version_official(binary: &Path, marker: &str) -> (bool, Option<String>) {
    match metadata_output(binary, &["--version"]) {
        Some(o) if o.status.success() => {
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
fn check_auth(binary: &Path, strategy: CliAuthStrategy) -> bool {
    match strategy {
        CliAuthStrategy::SubCommand(arg1, arg2) => {
            metadata_output(binary, &[arg1, arg2]).is_some_and(|output| output.status.success())
        }
        CliAuthStrategy::Unknown => false,
    }
}

/// Detect all CLI tools in a single pass.
pub fn detect_all_cli_providers() -> Vec<CliDetectionResult> {
    std::thread::scope(|scope| {
        let tasks: Vec<_> = ALL_CLI_VARIANTS
            .iter()
            .map(|variant| scope.spawn(move || variant.detect()))
            .collect();
        tasks
            .into_iter()
            .filter_map(|task| task.join().ok())
            .collect()
    })
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
    /// Native CLI authority is disabled for the family's default chat mode.
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
            CliVariant::ClaudeCode if self.permission_mode == CliPermissionMode::AllowListOnly => {
                "claude:chat-only-no-native-tools"
            }
            CliVariant::ClaudeCode if self.permission_mode == CliPermissionMode::Interactive => {
                "claude:default-permissions"
            }
            CliVariant::ClaudeCode => "claude:explicit-permission-bypass",
            CliVariant::OpenAiCodex => "codex:chat-only-isolated-app-server",
            CliVariant::GeminiCli => "gemini:no-runtime-permission-flag",
            CliVariant::XaiGrokCli => "grok:chat-only-isolated-acp",
        }
    }

    fn warn_if_permission_mode_not_effective(&self) {
        if self.variant != CliVariant::GeminiCli
            || self.permission_mode == CliPermissionMode::DangerousSkipAll
        {
            return;
        }
        if !CLI_PERMISSION_MODE_WARNED.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                configured_mode = ?self.permission_mode,
                variant = %self.variant,
                runtime_posture = %self.runtime_permission_posture(),
                "This CLI variant cannot enforce the family's chat-only permission mode"
            );
        }
    }

    /// Check whether the CLI binary is available on PATH (synchronous).
    pub fn is_available(&self) -> bool {
        resolve_cli_binary(self.variant)
            .ok()
            .and_then(|binary| metadata_output(&binary, &["--version"]))
            .is_some_and(|output| output.status.success())
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

    /// Claude's native tools are outside SkillExecutor's approval boundary.
    /// Disable them and discovered MCP servers for ordinary family chat.
    fn apply_permission_flags(&self, cmd: &mut Command) {
        match self.permission_mode {
            CliPermissionMode::AllowListOnly => {
                cmd.args([
                    "--tools",
                    "",
                    "--permission-mode",
                    "dontAsk",
                    "--no-session-persistence",
                    "--setting-sources",
                    "",
                    "--strict-mcp-config",
                    "--mcp-config",
                    "{\"mcpServers\":{}}",
                ]);
                let directory = std::env::temp_dir().join("abigail-cli-chat");
                if std::fs::create_dir_all(&directory).is_ok() {
                    cmd.current_dir(directory);
                }
            }
            CliPermissionMode::Interactive => {
                cmd.args(["--permission-mode", "default"]);
            }
            CliPermissionMode::DangerousSkipAll => {
                cmd.arg("--dangerously-skip-permissions");
            }
        }
        cmd.env_remove("CLAUDECODE")
            .env_remove("CLAUDE_CODE_ENTRYPOINT")
            .env_remove("CLAUDE_CODE_SESSION_ID");
        cmd.kill_on_drop(true);
    }

    fn apply_model_override(&self, cmd: &mut Command, model_override: Option<&str>) {
        let Some(model) = model_override.map(str::trim).filter(|m| !m.is_empty()) else {
            return;
        };

        if is_model_compatible_with_provider(self.variant.model_override_provider(), model) {
            tracing::info!(
                variant = %self.variant,
                model_override = %model,
                "Applying provider-native model override to CLI request"
            );
            cmd.arg("--model").arg(model);
        } else {
            tracing::warn!(
                variant = %self.variant,
                model_override = %model,
                "Dropping incompatible model override for CLI provider"
            );
        }
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
        model_override: Option<&str>,
    ) -> (Command, Option<String>) {
        let binary = resolve_cli_binary(self.variant)
            .unwrap_or_else(|_| PathBuf::from(self.variant.binary_name()));
        let mut cmd = Command::new(binary);

        self.warn_if_permission_mode_not_effective();

        if self.api_key != "system" {
            cmd.env(self.variant.api_key_env_var(), &self.api_key);
        }

        let stdin_content;

        match self.variant {
            CliVariant::ClaudeCode => {
                self.apply_model_override(&mut cmd, model_override);
                let has_session = if self.permission_mode == CliPermissionMode::AllowListOnly {
                    None
                } else {
                    self.active_session_id.read().ok().and_then(|g| g.clone())
                };

                if let Some(ref sid) = has_session {
                    // Resume existing session — Claude keeps the system
                    // prompt and conversation state. Only send new input.
                    cmd.arg("--resume").arg(sid);
                    cmd.arg("--print");
                    cmd.arg("--output-format").arg("json");
                    cmd.arg("--max-turns").arg("5");
                    self.apply_permission_flags(&mut cmd);
                    cmd.stdin(std::process::Stdio::piped());
                    stdin_content = Some(prompt.to_string());
                } else {
                    // First message — send system prompt + user prompt.
                    cmd.arg("--print");
                    cmd.arg("--output-format").arg("json");
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
                self.apply_model_override(&mut cmd, model_override);
                cmd.arg("--prompt");
                if let Some(sp) = system_prompt {
                    let tmp = std::env::temp_dir().join("abigail_gemini_system.md");
                    let _ = std::fs::write(&tmp, sp);
                    cmd.env("GEMINI_SYSTEM_MD", tmp);
                }
                cmd.arg(prompt);
                stdin_content = None;
            }
            CliVariant::OpenAiCodex | CliVariant::XaiGrokCli => {
                unreachable!("Native account providers use their isolated protocol adapters")
            }
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        hide_console_window_async(&mut cmd);
        (cmd, stdin_content)
    }

    async fn complete_via_cli(
        &self,
        request: &CompletionRequest,
    ) -> anyhow::Result<CompletionResponse> {
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

        let (cmd, stdin_content) = self.build_command(
            &prompt,
            system_prompt.as_deref(),
            request.model_override.as_deref(),
        );

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
            return Err(sanitized_cli_failure(self.variant, &stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if self.variant == CliVariant::ClaudeCode {
            let result: serde_json::Value = serde_json::from_str(stdout.trim())
                .map_err(|_| anyhow::anyhow!("Claude CLI did not return a valid completion; check its connection and authentication"))?;
            decode_claude_result(&result)
        } else if stdout.trim().is_empty() {
            Err(anyhow::anyhow!(
                "{} CLI returned an empty reply",
                self.variant
            ))
        } else {
            Ok(stdout.trim().to_string())
        }
    }
}

// Claude can exit successfully even when authentication or inference failed.
// Only an explicit successful result is a completion that may enter memory.
fn sanitized_cli_failure(variant: CliVariant, diagnostic: &str) -> anyhow::Error {
    let text = diagnostic.to_ascii_lowercase();
    let guidance = if [
        "authentication required",
        "failed to authenticate",
        "unauthorized",
        "invalid token",
        "token invalid",
        "expired token",
        "token expired",
        "please log in",
        "please sign in",
        "401",
    ]
    .iter()
    .any(|phrase| text.contains(phrase))
    {
        "authentication required or expired. Sign in again with the official CLI, then retry."
    } else if [
        "rate limit",
        "rate_limit",
        "usage limit",
        "quota",
        "insufficient credits",
        "credits exhausted",
        "429",
    ]
    .iter()
    .any(|phrase| text.contains(phrase))
    {
        "account usage limit reached. Wait for the limit to reset or choose another connection in Hive."
    } else if text.contains("model")
        && [
            "unavailable",
            "not available",
            "not found",
            "unsupported model",
            "unknown model",
        ]
        .iter()
        .any(|phrase| text.contains(phrase))
    {
        "selected model is unavailable. Choose another model or connection in Hive."
    } else {
        "connection failed. Check its account connection and selected model, then retry."
    };
    anyhow::anyhow!("{} CLI {}", variant, guidance)
}

fn decode_claude_result(result: &serde_json::Value) -> anyhow::Result<String> {
    if result.get("is_error").and_then(|value| value.as_bool()) == Some(true)
        || result.get("subtype").and_then(|value| value.as_str()) != Some("success")
    {
        let message = result
            .get("result")
            .and_then(|value| value.as_str())
            .filter(|message| !message.trim().is_empty())
            .unwrap_or("connection or inference failed; check Claude authentication");
        return Err(sanitized_cli_failure(CliVariant::ClaudeCode, message));
    }
    let content = result
        .get("result")
        .and_then(|value| value.as_str())
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Claude CLI returned an empty reply"))?;
    Ok(content.to_string())
}

#[async_trait]
impl LlmProvider for CliLlmProvider {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        match self.variant {
            CliVariant::OpenAiCodex => {
                return super::codex_cli::complete(&self.api_key, request).await
            }
            CliVariant::XaiGrokCli => {
                return super::grok_cli::complete(&self.api_key, request).await
            }
            _ => {}
        }
        if request.model_override.is_some() {
            return self.complete_via_cli(request).await;
        }

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

        let (cmd, stdin_content) = self.build_command(
            &prompt,
            system_prompt.as_deref(),
            request.model_override.as_deref(),
        );

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

        match self.variant {
            CliVariant::OpenAiCodex => {
                return super::codex_cli::stream(&self.api_key, request, tx).await
            }
            CliVariant::XaiGrokCli => {
                return super::grok_cli::stream(&self.api_key, request, tx).await
            }
            _ => {}
        }
        if self.variant != CliVariant::ClaudeCode {
            return self.complete(request).await.inspect(|resp| {
                let _ = tx.try_send(StreamEvent::Token(resp.content.clone()));
                let _ = tx.try_send(StreamEvent::Done(resp.clone()));
            });
        }

        let system_prompt = Self::extract_system_prompt(&request.messages);
        let prompt = Self::build_prompt(&request.messages);

        let has_session = if self.permission_mode == CliPermissionMode::AllowListOnly {
            None
        } else {
            self.active_session_id.read().ok().and_then(|g| g.clone())
        };

        let mut cmd = Command::new(resolve_cli_binary(self.variant)?);
        self.warn_if_permission_mode_not_effective();
        if self.api_key != "system" {
            cmd.env(self.variant.api_key_env_var(), &self.api_key);
        }
        self.apply_model_override(&mut cmd, request.model_override.as_deref());
        cmd.arg("--verbose").arg("--include-partial-messages");

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
        let stderr = child.stderr.take();
        let stderr_task = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut message = String::new();
            if let Some(mut stderr) = stderr {
                let _ = stderr.read_to_string(&mut message).await;
            }
            message
        });

        let mut reader = tokio::io::BufReader::new(stdout).lines();
        let mut full_content = String::new();
        let mut captured_session_id: Option<String> = None;
        let mut final_result = None;

        while let Ok(Some(line)) = reader.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
                if event.get("type").and_then(|value| value.as_str()) == Some("result") {
                    final_result = Some(decode_claude_result(&event));
                    continue;
                }
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
                    .get("event")
                    .and_then(|event| event.get("delta"))
                    .and_then(|delta| delta.get("text"))
                    .and_then(|text| text.as_str())
                    .or_else(|| {
                        event
                            .get("content_block_delta")
                            .or_else(|| event.get("delta"))
                            .and_then(|d| d.get("text"))
                            .and_then(|t| t.as_str())
                    })
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

        let status = child.wait().await?;
        let stderr = stderr_task.await.unwrap_or_default();
        if !status.success() {
            return Err(sanitized_cli_failure(CliVariant::ClaudeCode, &stderr));
        }
        full_content = final_result
            .ok_or_else(|| anyhow::anyhow!("Claude CLI stream ended without a completion"))??;

        // Store session ID for subsequent --resume calls.
        if let Some(sid) =
            captured_session_id.filter(|_| self.permission_mode != CliPermissionMode::AllowListOnly)
        {
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

    fn command_args(cmd: &Command) -> Vec<String> {
        cmd.as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

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
    fn native_resolver_does_not_launch_shell_wrappers() {
        let directory = std::env::temp_dir().join(format!(
            "abigail-cli-resolver-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let wrapper = directory.join("codex.cmd");
        std::fs::write(&wrapper, "do not execute").unwrap();
        assert!(resolve_from_directories(CliVariant::OpenAiCodex, &[directory.clone()]).is_none());
        let native = directory.join(if cfg!(windows) { "codex.exe" } else { "codex" });
        std::fs::write(&native, "native fixture").unwrap();
        assert_eq!(
            resolve_from_directories(CliVariant::OpenAiCodex, &[directory.clone()]),
            Some(native.clone())
        );
        std::fs::remove_file(native).unwrap();
        std::fs::remove_file(wrapper).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn native_resolver_finds_codex_optional_npm_package() {
        let directory = std::env::temp_dir().join(format!(
            "abigail-codex-npm-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let target = if cfg!(target_arch = "aarch64") {
            "aarch64-pc-windows-msvc"
        } else {
            "x86_64-pc-windows-msvc"
        };
        let package = if cfg!(target_arch = "aarch64") {
            "codex-win32-arm64"
        } else {
            "codex-win32-x64"
        };
        let native = directory.join(format!("node_modules/@openai/codex/node_modules/@openai/{package}/vendor/{target}/bin/codex.exe"));
        std::fs::create_dir_all(native.parent().unwrap()).unwrap();
        std::fs::write(&native, "native fixture").unwrap();
        assert_eq!(
            resolve_from_directories(CliVariant::OpenAiCodex, &[directory.clone()]),
            Some(native.clone())
        );
        std::fs::remove_file(&native).unwrap();
        let mut empty = native.parent().unwrap().to_path_buf();
        loop {
            std::fs::remove_dir(&empty).unwrap();
            if empty == directory {
                break;
            }
            empty = empty.parent().unwrap().to_path_buf();
        }
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

    #[test]
    fn test_build_command_applies_cli_model_override() {
        let provider = CliLlmProvider::new(CliVariant::ClaudeCode, "system".to_string()).unwrap();
        let (cmd, _) = provider.build_command("Hello", None, Some("claude-sonnet-4-6"));
        let args = command_args(&cmd);

        assert!(args
            .windows(2)
            .any(|pair| pair == ["--model", "claude-sonnet-4-6"]));
    }

    #[test]
    fn test_build_command_drops_incompatible_cli_model_override() {
        let provider = CliLlmProvider::new(CliVariant::ClaudeCode, "system".to_string()).unwrap();
        let (cmd, _) = provider.build_command("Hello", None, Some("gemini-2.5-pro"));
        let args = command_args(&cmd);

        assert!(!args.iter().any(|arg| arg == "--model"));
    }

    #[test]
    fn default_claude_chat_disables_native_tools_and_mcp_without_bypassing_permissions() {
        let provider = CliLlmProvider::new(CliVariant::ClaudeCode, "system".to_string()).unwrap();
        let (cmd, _) = provider.build_command("Hello", None, None);
        let args = command_args(&cmd);
        assert!(args.windows(2).any(|pair| pair == ["--tools", ""]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--permission-mode", "dontAsk"]));
        assert!(args.iter().any(|arg| arg == "--strict-mcp-config"));
        assert!(args.iter().any(|arg| arg == "--no-session-persistence"));
        assert!(!args
            .iter()
            .any(|arg| arg == "--dangerously-skip-permissions"));
    }

    #[test]
    fn claude_result_rejects_auth_errors_even_with_successful_process_exit() {
        let error = serde_json::json!({
            "type": "result", "subtype": "success", "is_error": true,
            "result": "Failed to authenticate. OAuth access token invalid"
        });
        assert!(decode_claude_result(&error).is_err());
        assert!(decode_claude_result(&serde_json::json!({
            "type": "result", "subtype": "error_max_turns", "result": "partial"
        }))
        .is_err());
        assert!(decode_claude_result(&serde_json::json!({
            "type": "result", "subtype": "success", "result": ""
        }))
        .is_err());
        assert_eq!(
            decode_claude_result(&serde_json::json!({
                "type": "result", "subtype": "success", "is_error": false,
                "result": "Hello from Abigail"
            }))
            .unwrap(),
            "Hello from Abigail"
        );
    }

    #[test]
    fn cli_failures_keep_recovery_categories_without_vendor_diagnostics() {
        for (message, category) in [
            (
                "Failed to authenticate: secret-account-path token invalid",
                "authentication",
            ),
            ("Rate limit reached: secret-account-path", "usage limit"),
            (
                "Model not found: secret-account-path",
                "model is unavailable",
            ),
            ("Internal error secret-account-path", "connection failed"),
        ] {
            let error = sanitized_cli_failure(CliVariant::ClaudeCode, message).to_string();
            assert!(error.contains(category));
            assert!(!error.contains("secret-account-path"));
            let result = serde_json::json!({"is_error":true,"result":message});
            assert_eq!(
                decode_claude_result(&result).unwrap_err().to_string(),
                error
            );
        }
    }
}
