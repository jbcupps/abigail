//! Grok Build ACP transport using the CLI's existing account or an explicit API key.
//!
//! Authentication stays inside the unmodified vendor CLI. Abigail never reads or
//! copies its credentials. Each request opens a new, chat-only ACP session in an
//! empty temporary directory; Abigail supplies the conversation history.
//!
//! Existing-account mode lets the CLI consume cached credentials internally;
//! it never calls `authenticate`, because `cached_token` can fall back to an
//! interactive login flow. Grok's AgentDefinition source documents exact `toolConfig`
//! with `injectDefaultTools: false`; an empty `tools` allowlist alone means all.
//! Native user extensions have no universal per-session off switch, so passive
//! `inspect --json` must show no active hooks, plugins, MCP or LSP before launch.

use super::cli_provider::{resolve_cli_binary, CliVariant};
use super::provider::{CompletionRequest, CompletionResponse, StreamEvent};
use anyhow::{bail, Context};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
const TURN_TIMEOUT: Duration = Duration::from_secs(180);
const AUTH_REQUIRED: &str =
    "Grok existing login is unavailable or expired. Sign in with `grok login` outside Abigail, then retry.";
static DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

fn authentication_method(
    api_key: &str,
    initialized: &Value,
) -> anyhow::Result<Option<&'static str>> {
    let methods = initialized
        .get("authMethods")
        .and_then(Value::as_array)
        .context("Grok Build did not report supported authentication methods")?;
    // initialize/session/new may use the CLI's existing account internally.
    // Explicit cached_token authentication can discard headless metadata and
    // fall back to interactive grok.com authentication when credentials expire.
    if api_key == "system" {
        return Ok(None);
    }
    if methods
        .iter()
        .any(|entry| entry.get("id").and_then(Value::as_str) == Some("xai.api_key"))
    {
        Ok(Some("xai.api_key"))
    } else {
        bail!("Grok Build cannot use the supplied API key");
    }
}

#[derive(Debug, PartialEq, Eq)]
enum RpcErrorCategory {
    Authentication,
    UsageLimit,
    ModelUnavailable,
    Rejected,
}

impl RpcErrorCategory {
    fn user_message(&self) -> &'static str {
        match self {
            Self::Authentication => AUTH_REQUIRED,
            Self::UsageLimit => "Grok account usage or rate limit reached. Check your Grok usage, then retry later.",
            Self::ModelUnavailable => "The selected Grok model is unavailable for this CLI account. Choose another available model and retry.",
            Self::Rejected => "Grok Build rejected the chat request; check the CLI connection and retry",
        }
    }
}

fn classify_rpc_error(error: &Value) -> RpcErrorCategory {
    // ACP -32000 is a generic server error. Classify only specific message
    // categories, then discard the vendor text rather than passing it to users.
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if [
        "authentication required",
        "not authenticated",
        "unauthenticated",
        "unauthorized",
        "failed to authenticate",
        "authentication failed",
        "login required",
        "please sign in",
        "sign in required",
        "please log in",
        "no cached auth token",
        "session expired",
        "expired credentials",
        "token expired",
        "expired token",
        "invalid token",
        "invalid oauth token",
    ]
    .iter()
    .any(|phrase| message.contains(phrase))
    {
        return RpcErrorCategory::Authentication;
    }
    if [
        "rate limit",
        "rate_limit",
        "ratelimit",
        "too many requests",
        "usage limit",
        "quota",
        "credits exhausted",
        "insufficient credits",
        "payment required",
        "subscription limit",
        "spending limit",
        "budget exceeded",
    ]
    .iter()
    .any(|phrase| message.contains(phrase))
    {
        return RpcErrorCategory::UsageLimit;
    }
    if message.contains("model")
        && [
            "unavailable",
            "not available",
            "not found",
            "unknown model",
            "unsupported model",
            "invalid model",
        ]
        .iter()
        .any(|phrase| message.contains(phrase))
    {
        return RpcErrorCategory::ModelUnavailable;
    }
    RpcErrorCategory::Rejected
}

struct RequestDirectory(PathBuf);

impl RequestDirectory {
    fn new() -> anyhow::Result<Self> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let counter = DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "abigail-grok-chat-{}-{stamp}-{counter}",
            std::process::id()
        ));
        // create_dir rejects a preexisting path, including a preexisting link.
        std::fs::create_dir(&path).context("Cannot create isolated Grok chat directory")?;
        Ok(Self(path))
    }
}

impl Drop for RequestDirectory {
    fn drop(&mut self) {
        // Only this request's newly created directory is ever removed.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn command(binary: &Path, directory: &Path, api_key: &str) -> Command {
    let mut cmd = Command::new(binary);
    cmd.current_dir(directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("GROK_DISABLE_AUTOUPDATER", "1")
        .env("GROK_MEMORY", "0")
        .env("GROK_SUBAGENTS", "0")
        .env("GROK_WRITE_FILE", "0")
        .env("GROK_WEB_FETCH", "0")
        .env("GROK_TOOL_SEARCH", "0")
        .env("GROK_LSP_TOOLS", "0")
        .env("GROK_AGENT", "grok-build")
        .env_remove("GROK_CONFIG")
        .env_remove("GROK_CONFIG_PATH");
    for vendor in ["CURSOR", "CLAUDE", "CODEX"] {
        for surface in ["SKILLS", "RULES", "AGENTS", "MCPS", "HOOKS", "SESSIONS"] {
            cmd.env(format!("GROK_{vendor}_{surface}_ENABLED"), "0");
        }
    }
    if api_key == "system" {
        // Existing-account mode must not silently switch to an inherited API key.
        cmd.env_remove("XAI_API_KEY");
    } else {
        cmd.env("XAI_API_KEY", api_key);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    cmd
}

fn validate_inspection(report: &Value) -> anyhow::Result<()> {
    for key in ["hooks", "plugins", "mcpServers", "lspServers"] {
        let entries = report.get(key).and_then(Value::as_array).ok_or_else(|| {
            anyhow::anyhow!(
                "Grok isolation preflight cannot verify the installed CLI's extension schema"
            )
        })?;
        for entry in entries {
            if !entry.is_object() {
                bail!("Grok isolation preflight received an invalid extension entry");
            }
            let disabled = entry.get("disabled").and_then(Value::as_bool) == Some(true)
                || entry.get("enabled").and_then(Value::as_bool) == Some(false)
                || (key == "lspServers"
                    && entry.get("untrusted").and_then(Value::as_bool) == Some(true));
            if !disabled {
                bail!(
                    "Grok cannot isolate family chat while configured native hooks, plugins, MCP or LSP are active. Use a Grok profile without those extensions, then retry."
                );
            }
        }
    }
    Ok(())
}

struct StderrDrain(JoinHandle<()>);

impl StderrDrain {
    fn start(child: &mut Child) -> Self {
        let stderr = child.stderr.take();
        Self(tokio::spawn(async move {
            if let Some(mut stderr) = stderr {
                // Drain without retaining or exposing vendor diagnostics/credentials.
                let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
            }
        }))
    }
}

impl Drop for StderrDrain {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn inspect(binary: &Path, directory: &Path, api_key: &str) -> anyhow::Result<()> {
    let mut cmd = command(binary, directory, api_key);
    cmd.args(["inspect", "--json"]).stdin(Stdio::null());
    let mut child = cmd
        .spawn()
        .context("Cannot start Grok isolation preflight")?;
    let _stderr = StderrDrain::start(&mut child);
    let stdout = child
        .stdout
        .take()
        .context("Cannot inspect Grok isolation")?;
    let checked = tokio::time::timeout(Duration::from_secs(15), async {
        let mut bytes = Vec::new();
        stdout
            .take((MAX_FRAME_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > MAX_FRAME_BYTES {
            bail!("Grok isolation preflight exceeded its output limit");
        }
        let status = child.wait().await?;
        if !status.success() {
            bail!("Grok isolation preflight failed; update Grok Build and retry");
        }
        let report: Value = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Grok isolation preflight returned invalid JSON"))?;
        validate_inspection(&report)
    })
    .await;
    match checked {
        Ok(result) => result,
        Err(_) => bail!("Grok isolation preflight timed out"),
    }
}

fn chat_profile() -> Value {
    json!({
        "name": "abigail-chat",
        "description": "Family chat with no native capabilities",
        "toolConfig": {"tools": []},
        "injectDefaultTools": false,
        "discoverSkills": false,
        "agentsMd": false,
        "permissionMode": "dontAsk",
        "mcpServers": [],
        "mcpInheritance": "none",
        "maxTurns": 1
    })
}

fn initialize_params(request: &CompletionRequest) -> Value {
    let system_prompt = request
        .messages
        .iter()
        .filter(|message| message.role == "system")
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    json!({
        "protocolVersion": 1,
        "clientInfo": {"name": "abigail-chat", "version": env!("CARGO_PKG_VERSION")},
        "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
        "_meta": {
            "clientType": "abigail",
            "systemPromptOverride": system_prompt,
            "startupHints": {"nonInteractive": true, "permissionMode": "dontAsk"}
        }
    })
}

#[derive(Default)]
struct TurnState {
    tools_verified: bool,
    prompt_started: bool,
    session_id: Option<String>,
    content: String,
}

impl TurnState {
    /// Return only ordinary assistant text; reasoning and vendor metadata stay private.
    fn notification(&mut self, message: &Value) -> anyhow::Result<Option<String>> {
        if message.get("method").and_then(Value::as_str) != Some("session/update") {
            return Ok(None);
        }
        let params = message
            .get("params")
            .context("Grok returned an invalid session update")?;
        if let Some(expected) = &self.session_id {
            if params.get("sessionId").and_then(Value::as_str) != Some(expected) {
                bail!("Grok returned an update for another chat session");
            }
        }
        let update = params
            .get("update")
            .context("Grok returned an invalid session update")?;
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("available_commands_update") => {
                let tools = update
                    .pointer("/_meta/tools")
                    .and_then(Value::as_array)
                    .context("Grok isolation cannot verify the native toolset")?;
                if !tools.is_empty() {
                    bail!("Grok isolation refused an enabled native toolset");
                }
                self.tools_verified = true;
                Ok(None)
            }
            Some("tool_call" | "tool_call_update") => {
                bail!("Grok isolation refused native tool activity")
            }
            Some("agent_message_chunk") => {
                if !self.prompt_started || !self.tools_verified {
                    bail!("Grok returned text before isolated chat was ready");
                }
                if update.pointer("/content/type").and_then(Value::as_str) != Some("text") {
                    bail!("Grok returned unsupported assistant content");
                }
                let delta = update
                    .pointer("/content/text")
                    .and_then(Value::as_str)
                    .context("Grok returned an invalid assistant text chunk")?;
                if self.content.len().saturating_add(delta.len()) > MAX_TEXT_BYTES {
                    bail!("Grok chat exceeded its response limit");
                }
                self.content.push_str(delta);
                Ok(Some(delta.to_owned()))
            }
            _ => Ok(None),
        }
    }
}

struct AcpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    _stderr: StderrDrain,
    next_id: u64,
    state: TurnState,
    tx: Option<Sender<StreamEvent>>,
}

impl AcpClient {
    fn start(
        binary: &Path,
        directory: &Path,
        api_key: &str,
        tx: Option<Sender<StreamEvent>>,
    ) -> anyhow::Result<Self> {
        let mut cmd = command(binary, directory, api_key);
        // Never attach to or modify the user's existing shared leader process.
        cmd.args(["agent", "--no-leader", "stdio"]);
        let mut child = cmd.spawn().context("Cannot start Grok Build")?;
        let stdin = child.stdin.take().context("Cannot open Grok ACP input")?;
        let stdout = child.stdout.take().context("Cannot open Grok ACP output")?;
        let stderr = StderrDrain::start(&mut child);
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            _stderr: stderr,
            next_id: 1,
            state: TurnState::default(),
            tx,
        })
    }

    async fn write(&mut self, value: &Value) -> anyhow::Result<()> {
        let mut bytes = serde_json::to_vec(value)?;
        if bytes.len() > MAX_TEXT_BYTES {
            bail!("Grok chat exceeded its request limit");
        }
        bytes.push(b'\n');
        self.stdin
            .write_all(&bytes)
            .await
            .context("Grok ACP input closed")?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn read(&mut self) -> anyhow::Result<Value> {
        let mut frame = Vec::new();
        loop {
            let buffer = self.stdout.fill_buf().await?;
            if buffer.is_empty() {
                bail!("Grok Build exited before chat completed");
            }
            let newline = buffer.iter().position(|byte| *byte == b'\n');
            let count = newline.map_or(buffer.len(), |index| index + 1);
            if frame.len().saturating_add(count) > MAX_FRAME_BYTES {
                bail!("Grok ACP exceeded its frame limit");
            }
            frame.extend_from_slice(&buffer[..count]);
            self.stdout.consume(count);
            if newline.is_some() {
                if frame.iter().all(u8::is_ascii_whitespace) {
                    frame.clear();
                    continue;
                }
                let value: Value = serde_json::from_slice(&frame)
                    .map_err(|_| anyhow::anyhow!("Grok returned invalid ACP JSON"))?;
                if !value.is_object() || value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
                {
                    bail!("Grok returned an invalid ACP envelope");
                }
                return Ok(value);
            }
        }
    }

    async fn accept_notification(&mut self, message: &Value) -> anyhow::Result<()> {
        if let Some(delta) = self.state.notification(message)? {
            if let Some(tx) = &self.tx {
                tx.send(StreamEvent::Token(delta))
                    .await
                    .map_err(|_| anyhow::anyhow!("Grok chat stream was cancelled"))?;
            }
        }
        Ok(())
    }

    async fn refuse_client_request(&mut self, message: &Value) -> anyhow::Result<()> {
        let id = message.get("id").context("Invalid Grok client request")?;
        let reply = if message.get("method").and_then(Value::as_str)
            == Some("session/request_permission")
        {
            json!({"jsonrpc":"2.0","id":id,"result":{"outcome":{"outcome":"cancelled"}}})
        } else {
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Abigail chat does not permit native client tools"}})
        };
        self.write(&reply).await?;
        bail!("Grok isolation refused a native client request")
    }

    async fn request(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.write(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        loop {
            let message = self.read().await?;
            if message.get("method").is_some() {
                if message.get("id").is_some() {
                    return self
                        .refuse_client_request(&message)
                        .await
                        .map(|_| Value::Null);
                }
                self.accept_notification(&message).await?;
                continue;
            }
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                bail!("Grok returned an unexpected ACP response");
            }
            if let Some(error) = message.get("error") {
                bail!(classify_rpc_error(error).user_message());
            }
            return message
                .get("result")
                .cloned()
                .context("Grok returned no ACP result");
        }
    }

    async fn verify_tools(&mut self) -> anyhow::Result<()> {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !self.state.tools_verified {
                let message = self.read().await?;
                if message.get("method").is_none() {
                    bail!("Unexpected Grok toolset response");
                }
                if message.get("id").is_some() {
                    return self.refuse_client_request(&message).await;
                }
                self.accept_notification(&message).await?;
            }
            Ok(())
        })
        .await
        .map_err(|_| anyhow::anyhow!("Grok isolation could not confirm an empty native toolset"))?
    }

    async fn shutdown(&mut self) {
        let _ = self.stdin.shutdown().await;
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
    }
}

async fn run(
    api_key: &str,
    request: &CompletionRequest,
    tx: Option<Sender<StreamEvent>>,
) -> anyhow::Result<CompletionResponse> {
    if api_key.trim().is_empty() {
        bail!("Grok CLI authentication mode must not be empty");
    }
    let directory = RequestDirectory::new()?;
    let binary = resolve_cli_binary(CliVariant::XaiGrokCli)?;
    inspect(&binary, &directory.0, api_key).await?;
    let mut client = AcpClient::start(&binary, &directory.0, api_key, tx)?;
    let result = async {
        let initialized = client.request("initialize", initialize_params(request)).await?;
        if let Some(method) = authentication_method(api_key, &initialized)? {
            client.request("authenticate", json!({"methodId":method,"_meta":{"headless":true}})).await?;
        }
        let opened = client.request("session/new", json!({
            "cwd":directory.0,
            "mcpServers":[],
            "_meta":{"sessionKind":"headless","nonInteractive":true,"agentProfile":chat_profile(),"pluginDirs":[]}
        })).await?;
        let session_id = opened.get("sessionId").and_then(Value::as_str)
            .filter(|id| !id.is_empty()).context("Grok Build did not open a chat session")?.to_owned();
        client.state.session_id = Some(session_id.clone());
        client.verify_tools().await?;
        if let Some(model) = request.model_override.as_deref().map(str::trim).filter(|model| !model.is_empty()) {
            if super::validation::is_model_compatible_with_provider("grok-cli", model) {
                client.request("session/set_model", json!({"sessionId":session_id,"modelId":model})).await?;
            }
        }
        let messages = request.messages.iter().filter(|message| message.role != "system").collect::<Vec<_>>();
        let prompt = serde_json::to_string(&messages)?;
        if prompt.len() > MAX_TEXT_BYTES { bail!("Grok chat exceeded its request limit"); }
        client.state.prompt_started = true;
        let final_result = client.request("session/prompt", json!({
            "sessionId":session_id,
            "prompt":[{"type":"text","text":format!("Conversation messages (respond to the final user message):\n{prompt}")}]
        })).await?;
        if final_result.get("stopReason").and_then(Value::as_str) != Some("end_turn") {
            bail!("Grok chat did not finish successfully");
        }
        if client.state.content.trim().is_empty() { bail!("Grok chat completed without assistant text"); }
        let response = CompletionResponse { content: client.state.content.clone(), tool_calls: None };
        if let Some(tx) = &client.tx {
            tx.send(StreamEvent::Done(response.clone())).await
                .map_err(|_| anyhow::anyhow!("Grok chat stream was cancelled"))?;
        }
        Ok(response)
    }.await;
    client.shutdown().await;
    result
}

pub async fn complete(
    api_key: &str,
    request: &CompletionRequest,
) -> anyhow::Result<CompletionResponse> {
    tokio::time::timeout(TURN_TIMEOUT, run(api_key, request, None))
        .await
        .map_err(|_| anyhow::anyhow!("Grok chat timed out"))?
}

pub async fn stream(
    api_key: &str,
    request: &CompletionRequest,
    tx: Sender<StreamEvent>,
) -> anyhow::Result<CompletionResponse> {
    tokio::time::timeout(TURN_TIMEOUT, run(api_key, request, Some(tx)))
        .await
        .map_err(|_| anyhow::anyhow!("Grok chat timed out"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_inspection() -> Value {
        json!({"hooks":[],"plugins":[],"mcpServers":[],"lspServers":[]})
    }

    #[test]
    fn existing_account_never_selects_explicit_authentication() {
        for methods in [
            json!([]),
            json!([{"id":"grok.com"}]),
            json!([{"id":"cached_token"},{"id":"grok.com"},{"id":"xai.api_key"}]),
        ] {
            assert_eq!(
                authentication_method("system", &json!({"authMethods":methods})).unwrap(),
                None
            );
        }
    }

    #[test]
    fn explicit_api_key_requires_its_advertised_authentication_method() {
        assert_eq!(
            authentication_method(
                "test-key",
                &json!({"authMethods":[{"id":"xai.api_key"},{"id":"grok.com"}]})
            )
            .unwrap(),
            Some("xai.api_key")
        );
        assert!(authentication_method(
            "test-key",
            &json!({"authMethods":[{"id":"cached_token"},{"id":"grok.com"}]})
        )
        .is_err());
        assert!(authentication_method("system", &json!({})).is_err());
    }

    #[test]
    fn generic_acp_server_error_is_not_an_authentication_failure() {
        for error in [
            json!({"code":-32000,"message":"Internal server error"}),
            json!({"code":-32000}),
            json!({"code":-32000,"message":"Maximum output tokens exceeded"}),
            json!({"code":-32000,"message":"Login service is temporarily unavailable"}),
        ] {
            assert_eq!(classify_rpc_error(&error), RpcErrorCategory::Rejected);
        }
    }

    #[test]
    fn rpc_categories_distinguish_authentication_models_and_usage_limits() {
        for message in [
            "Authentication required",
            "No cached auth token found",
            "OAuth token expired",
        ] {
            assert_eq!(
                classify_rpc_error(&json!({"code":-32000,"message":message})),
                RpcErrorCategory::Authentication
            );
        }
        for message in [
            "Model unavailable",
            "Unknown model grok-missing",
            "The model is not available on this plan",
        ] {
            assert_eq!(
                classify_rpc_error(&json!({"code":-32000,"message":message})),
                RpcErrorCategory::ModelUnavailable
            );
        }
        for message in [
            "Usage limit exceeded",
            "Rate limit reached",
            "Token quota exhausted",
            "Too many requests",
        ] {
            let category = classify_rpc_error(&json!({"code":-32000,"message":message}));
            assert_eq!(category, RpcErrorCategory::UsageLimit);
            assert!(!category.user_message().contains("grok login"));
        }
    }

    #[test]
    fn rpc_diagnostics_never_enter_the_user_facing_message() {
        let error = json!({
            "code":-32000,
            "message":"Authentication required: secret-diagnostic-value",
            "data":{"token":"secret-diagnostic-value"}
        });
        let category = classify_rpc_error(&error);
        assert_eq!(category, RpcErrorCategory::Authentication);
        assert_eq!(category.user_message(), AUTH_REQUIRED);
        assert!(!category.user_message().contains("secret-diagnostic-value"));
    }

    #[test]
    fn isolation_rejects_active_extensions_and_unknown_schema() {
        assert!(validate_inspection(&empty_inspection()).is_ok());
        for key in ["hooks", "plugins", "mcpServers", "lspServers"] {
            let mut report = empty_inspection();
            report[key] = json!([{}]);
            assert!(validate_inspection(&report).is_err(), "{key}");
            report[key] = json!([{"disabled":true}]);
            assert!(validate_inspection(&report).is_ok());
            report.as_object_mut().unwrap().remove(key);
            assert!(validate_inspection(&report).is_err());
        }
    }

    #[test]
    fn isolated_profile_prevents_default_tool_and_skill_injection() {
        let profile = chat_profile();
        assert_eq!(profile["toolConfig"]["tools"], json!([]));
        for key in ["injectDefaultTools", "discoverSkills", "agentsMd"] {
            assert_eq!(profile[key], false);
        }
        assert_eq!(profile["permissionMode"], "dontAsk");
        assert_eq!(profile["mcpInheritance"], "none");
    }

    #[test]
    fn native_tools_must_be_confirmed_empty_before_any_text() {
        let mut state = TurnState::default();
        let chunk = json!({"method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hello"}}}});
        assert!(state.notification(&chunk).is_err());
        let mut capabilities = json!({"method":"session/update","params":{"update":{"sessionUpdate":"available_commands_update","_meta":{"tools":["bash"]}}}});
        assert!(state.notification(&capabilities).is_err());
        capabilities["params"]["update"]["_meta"]["tools"] = json!([]);
        assert!(state.notification(&capabilities).is_ok());
        state.prompt_started = true;
        state.session_id = Some("s".into());
        assert_eq!(state.notification(&chunk).unwrap(), Some("hello".into()));
        let call = json!({"method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call"}}});
        assert!(state.notification(&call).is_err());
    }

    #[test]
    fn stream_rejects_other_session_and_ignores_private_reasoning() {
        let mut state = TurnState {
            session_id: Some("alpha".into()),
            tools_verified: true,
            prompt_started: true,
            ..Default::default()
        };
        let mut event = json!({"method":"session/update","params":{"sessionId":"beta","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"private"}}}});
        assert!(state.notification(&event).is_err());
        event["params"]["sessionId"] = json!("alpha");
        event["params"]["update"]["sessionUpdate"] = json!("agent_thought_chunk");
        assert!(state.notification(&event).unwrap().is_none());
        assert!(state.content.is_empty());
    }
}
