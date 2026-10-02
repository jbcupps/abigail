//! Owned Codex app-server sessions using the CLI's existing account.
//!
//! No execution environment is selected, so filesystem/shell handlers are absent.
//! Hooks, plugins and MCP are disabled before a turn, and unexpected tool protocol
//! messages fail closed. Account tokens stay inside Codex; this client never reads
//! credential files or uses login/config-write RPCs.

use super::cli_provider::{resolve_cli_binary, CliVariant};
use super::provider::{CompletionRequest, CompletionResponse, StreamEvent};
use anyhow::{bail, Result};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc::Sender;

const MAX_FRAME: usize = 4 * 1024 * 1024;
const MAX_CONTENT: usize = 4 * 1024 * 1024;
const SESSION_TIMEOUT: Duration = Duration::from_secs(120);
const BASE: &str = "You are a helpful conversation assistant inside Abigail. Respond to the supplied conversation. You have no external tools or execution environment.";
const DISABLED_FEATURES: &[&str] = &[
    "shell_tool",
    "unified_exec",
    "view_image",
    "plugins",
    "hooks",
    "apps",
    "multi_agent",
    "multi_agent_v2",
    "code_mode",
    "code_mode_only",
    "code_mode_host",
    "js_repl",
    "search_tool",
    "tool_search",
    "tool_suggest",
    "recommended_plugins",
    "remote_plugin",
    "memories",
    "skill_search",
    "skill_mcp_dependency_install",
    "skill_env_var_dependency_prompt",
    "browser_use",
    "computer_use",
    "deferred_executor",
    "current_time_reminder",
    "sleep_tool",
    "token_budget",
    "request_permissions_tool",
    "enable_mcp_apps",
    "image_generation",
    "standalone_web_search",
    "artifact",
];

pub(crate) async fn complete(key: &str, request: &CompletionRequest) -> Result<CompletionResponse> {
    run(key, request, None).await
}

pub(crate) async fn stream(
    key: &str,
    request: &CompletionRequest,
    tx: Sender<StreamEvent>,
) -> Result<CompletionResponse> {
    run(key, request, Some(tx)).await
}

async fn run(
    key: &str,
    request: &CompletionRequest,
    tx: Option<Sender<StreamEvent>>,
) -> Result<CompletionResponse> {
    if key != "system" {
        bail!("Codex existing-account chat requires your installed Codex sign-in. Use the OpenAI API provider for an API key.");
    }
    tokio::time::timeout(SESSION_TIMEOUT, run_session(request, tx))
        .await
        .map_err(|_| anyhow::anyhow!("Codex chat timed out; its isolated session was stopped. Retry or check your Codex connection."))?
}

struct Scratch(PathBuf);

impl Scratch {
    fn create() -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "abigail-codex-{}-{}-{}",
            std::process::id(),
            time,
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).map_err(|_| {
            anyhow::anyhow!("Codex isolation could not create a private working folder.")
        })?;
        let scratch = Self(path);
        std::fs::write(scratch.0.join("instructions.txt"), BASE)
            .map_err(|_| anyhow::anyhow!("Codex isolation could not create its instructions."))?;
        Ok(scratch)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Only this invocation's newly created directory is ever removed.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn isolation_config(scratch: &Path) -> Map<String, Value> {
    let mut settings = Map::new();
    for feature in DISABLED_FEATURES {
        settings.insert(format!("features.{feature}"), json!(false));
    }
    for (key, value) in [
        ("features.skip_host_skill_discovery", json!(true)),
        ("notify", json!([])),
        ("web_search", json!("disabled")),
        ("model_provider", json!("openai")),
        ("project_doc_max_bytes", json!(0)),
        (
            "model_instructions_file",
            json!(scratch.join("instructions.txt")),
        ),
        (
            "developer_instructions",
            json!("Respond only to the conversation supplied by Abigail."),
        ),
        ("include_apps_instructions", json!(false)),
        ("include_environment_context", json!(false)),
        ("include_collaboration_mode_instructions", json!(false)),
        ("skills.include_instructions", json!(false)),
        ("orchestrator.skills.enabled", json!(false)),
        ("orchestrator.mcp.enabled", json!(false)),
        ("tools.update_plan.enabled", json!(false)),
        (
            "tools.experimental_request_user_input.enabled",
            json!(false),
        ),
        ("agents.enabled", json!(false)),
    ] {
        settings.insert(key.into(), value);
    }
    settings
}

fn disable_inherited_extensions(config: &Value, settings: &mut Map<String, Value>) -> Result<()> {
    for section in ["mcp_servers", "plugins"] {
        if let Some(entries) = config.get(section).and_then(Value::as_object) {
            if entries.len() > 1024 {
                bail!("Codex isolation cannot validate its extension configuration.");
            }
            let mut disabled = Map::new();
            for name in entries.keys() {
                if name.len() > 1024 || name.chars().any(char::is_control) {
                    bail!("Codex isolation cannot validate an extension name.");
                }
                disabled.insert(name.clone(), json!({"enabled":false}));
            }
            // CLI dotted override paths do not interpret quoted key components.
            // A TOML inline table preserves arbitrary names while merging each
            // explicit enabled=false into its inherited connection settings.
            settings.insert(section.into(), Value::Object(disabled));
        }
    }
    Ok(())
}

fn toml_value(value: &Value) -> String {
    match value {
        Value::Object(entries) => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(name, value)| {
                    format!(
                        "{}={}",
                        serde_json::to_string(name).expect("string encoding"),
                        toml_value(value)
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(toml_value).collect::<Vec<_>>().join(",")
        ),
        _ => value.to_string(),
    }
}

fn verify_config(config: &Value) -> Result<()> {
    let features = config
        .get("features")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            anyhow::anyhow!("Codex isolation requires a CLI with reflected tool settings.")
        })?;
    for feature in [
        "shell_tool",
        "plugins",
        "hooks",
        "apps",
        "multi_agent",
        "code_mode",
        "js_repl",
        "image_generation",
        "standalone_web_search",
        "artifact",
    ] {
        if features.get(feature) != Some(&json!(false)) {
            bail!("Codex isolation could not disable native tools or extensions. Update Codex or use a profile without active extensions.");
        }
    }
    if config.get("web_search") != Some(&json!("disabled"))
        || config.get("notify") != Some(&json!([]))
        || config.get("model_provider") != Some(&json!("openai"))
    {
        bail!("Codex isolation settings were not applied.");
    }
    for section in ["mcp_servers", "plugins"] {
        if let Some(entries) = config.get(section).and_then(Value::as_object) {
            if entries
                .values()
                .any(|entry| entry.get("enabled") != Some(&json!(false)))
            {
                bail!("Codex isolation could not disable configured native MCP or plugins.");
            }
        }
    }
    if config
        .get("model_providers")
        .and_then(|v| v.get("openai"))
        .is_some()
    {
        bail!("Codex isolation does not support overriding its built-in OpenAI provider.");
    }
    Ok(())
}

struct Session {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next_id: u64,
    deferred: Vec<Value>,
    deferred_bytes: usize,
}

impl Session {
    fn spawn(binary: &Path, scratch: &Path, settings: &Map<String, Value>) -> Result<Self> {
        let mut command = Command::new(binary);
        command.args(["app-server", "--stdio", "--strict-config"]);
        for (key, value) in settings {
            command
                .arg("-c")
                .arg(format!("{key}={}", toml_value(value)));
        }
        command
            .current_dir(scratch)
            .env("CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED", "1")
            .env_remove("OPENAI_API_KEY")
            .env_remove("OPENAI_BASE_URL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        let mut child = command.spawn().map_err(|_| {
            anyhow::anyhow!(
                "Could not start installed Codex. Update Codex and check its connection."
            )
        })?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Codex session input is unavailable."))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Codex session output is unavailable."))?;
        Ok(Self {
            child,
            input,
            output: BufReader::new(output),
            next_id: 0,
            deferred: Vec::new(),
            deferred_bytes: 0,
        })
    }

    async fn stop(&mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }

    async fn send(&mut self, message: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&message)
            .map_err(|_| anyhow::anyhow!("Codex request could not be encoded."))?;
        bytes.push(b'\n');
        self.input
            .write_all(&bytes)
            .await
            .map_err(|_| anyhow::anyhow!("Codex session closed before completion."))?;
        self.input
            .flush()
            .await
            .map_err(|_| anyhow::anyhow!("Codex session closed before completion."))?;
        Ok(())
    }

    async fn read(&mut self) -> Result<Value> {
        let mut line = Vec::new();
        loop {
            let available = self
                .output
                .fill_buf()
                .await
                .map_err(|_| anyhow::anyhow!("Codex session output could not be read."))?;
            if available.is_empty() {
                bail!(
                    "Codex session ended before a completed turn. Check its connection and retry."
                );
            }
            let end = available.iter().position(|b| *b == b'\n');
            let count = end.map_or(available.len(), |n| n + 1);
            if line.len() + count > MAX_FRAME {
                bail!("Codex session returned an oversized protocol message.");
            }
            line.extend_from_slice(&available[..count]);
            self.output.consume(count);
            if end.is_some() {
                break;
            }
        }
        let value: Value = serde_json::from_slice(&line)
            .map_err(|_| anyhow::anyhow!("Codex returned an unsupported protocol message."))?;
        if let Some(denial) = native_request_denial(&value) {
            self.send(denial).await?;
            bail!("Codex isolation blocked an unexpected native tool or approval request.");
        }
        reject_unsafe_notification(&value)?;
        Ok(value)
    }

    async fn rpc(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"id":id,"method":method,"params":params}))
            .await?;
        loop {
            let message = self.read().await?;
            if message.get("id") == Some(&json!(id)) {
                if message.get("error").is_some() {
                    bail!("{}", safe_failure(&message["error"]));
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("Codex returned an invalid RPC response."));
            }
            self.deferred_bytes += serde_json::to_vec(&message).map_or(MAX_FRAME, |v| v.len());
            if self.deferred.len() >= 2048 || self.deferred_bytes > MAX_FRAME * 2 {
                bail!(
                    "Codex returned too many protocol messages before acknowledging the request."
                );
            }
            self.deferred.push(message);
        }
    }

    async fn initialize(&mut self) -> Result<()> {
        self.rpc("initialize", json!({"clientInfo":{"name":"abigail_chat","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}})).await?;
        self.send(json!({"method":"initialized","params":{}})).await
    }
}

fn native_request_denial(message: &Value) -> Option<Value> {
    if message.get("method").is_some() && message.get("id").is_some() {
        Some(
            json!({"id":message["id"],"error":{"code":-32601,"message":"Abigail chat disables all native tools and approvals"}}),
        )
    } else {
        None
    }
}

fn reject_unsafe_notification(message: &Value) -> Result<()> {
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    if matches!(method, "item/started" | "item/completed") {
        let item_type = message
            .pointer("/params/item/type")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !matches!(item_type, "userMessage" | "agentMessage" | "reasoning") {
            bail!("Codex isolation blocked an unexpected native tool item.");
        }
    }
    if method.starts_with("item/commandExecution/")
        || method.starts_with("item/fileChange/")
        || method.starts_with("item/mcpToolCall/")
        || method.starts_with("hook/")
    {
        bail!("Codex isolation blocked native tool activity.");
    }
    if method == "error" {
        bail!("{}", safe_failure(&message["params"]));
    }
    if method == "configWarning" {
        bail!("Codex could not run with verified isolation. Update Codex or check your existing connection.");
    }
    Ok(())
}

/// Inspect diagnostics locally and expose only fixed messages. CLI payloads may
/// contain file paths, HTTP headers, prompts, or tokens and must never be returned.
fn safe_failure(diagnostic: &Value) -> &'static str {
    let text = diagnostic.to_string().to_ascii_lowercase();
    if [
        "401",
        "unauthorized",
        "authentication",
        "authenticate",
        "auth_required",
        "authrequired",
        "token expired",
        "tokenexpired",
        "invalid token",
        "invalid_token",
        "not logged in",
        "login required",
        "sign in",
    ]
    .iter()
    .any(|pattern| text.contains(pattern))
    {
        "Codex authentication failed. Check your existing Codex sign-in, then reconnect it in Hive."
    } else if [
        "429",
        "rate_limit",
        "rate limit",
        "ratelimit",
        "usage limit",
        "usagelimit",
        "quota",
        "insufficient credit",
        "insufficient_credit",
        "credits",
    ]
    .iter()
    .any(|pattern| text.contains(pattern))
    {
        "Codex account usage limit reached. Check your plan's availability and retry later."
    } else if (text.contains("model")
        && [
            "invalid",
            "not supported",
            "does not exist",
            "unavailable",
            "not found",
        ]
        .iter()
        .any(|pattern| text.contains(pattern)))
        || [
            "model_not_found",
            "model not found",
            "model unavailable",
            "unsupported model",
            "model does not exist",
            "model_not_available",
        ]
        .iter()
        .any(|pattern| text.contains(pattern))
    {
        "The selected Codex model is unavailable for this account. Choose another model in Hive."
    } else {
        "Codex could not complete the chat turn. Check its connection and retry."
    }
}

async fn run_session(
    request: &CompletionRequest,
    tx: Option<Sender<StreamEvent>>,
) -> Result<CompletionResponse> {
    let binary = resolve_cli_binary(CliVariant::OpenAiCodex)?;
    let scratch = Scratch::create()?;
    let mut settings = isolation_config(&scratch.0);
    // Map overrides merge in Codex. Enumerate configuration without creating a
    // thread, then explicitly disable every inherited entry in a fresh process.
    let mut preflight = Session::spawn(&binary, &scratch.0, &settings)?;
    preflight.initialize().await?;
    let config = preflight
        .rpc("config/read", json!({"includeLayers":false}))
        .await?;
    disable_inherited_extensions(&config["config"], &mut settings)?;
    preflight.stop().await;
    let mut session = Session::spawn(&binary, &scratch.0, &settings)?;
    session.initialize().await?;
    let config = session
        .rpc("config/read", json!({"includeLayers":false}))
        .await?;
    verify_config(&config["config"])?;
    let model_list = session.rpc("model/list", json!({})).await?;
    let model = choose_model(&model_list, request.model_override.as_deref())?;
    let developer = request
        .messages
        .iter()
        .filter(|m| m.role == "system" || m.role == "developer")
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut start = json!({"ephemeral":true,"environments":[],"selectedCapabilityRoots":[],"dynamicTools":[],"sandbox":"read-only","approvalPolicy":"never","baseInstructions":BASE,"developerInstructions":format!("Respond only to the conversation supplied by Abigail.\n\n{developer}"),"cwd":scratch.0});
    start["model"] = json!(model);
    let thread = session.rpc("thread/start", start).await?;
    if thread.get("instructionSources") != Some(&json!([]))
        || thread.pointer("/sandbox/type") != Some(&json!("readOnly"))
        || thread.pointer("/sandbox/networkAccess") != Some(&json!(false))
    {
        bail!("Codex isolation could not verify an instruction-free, read-only thread.");
    }
    let thread_id = thread
        .pointer("/thread/id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Codex did not provide an isolated thread identifier."))?
        .to_owned();
    let conversation: Vec<_> = request
        .messages
        .iter()
        .filter(|m| m.role != "system" && m.role != "developer")
        .map(|m| json!({"role":m.role,"content":m.content}))
        .collect();
    let text = format!("Continue this conversation and reply to its latest user message. The following JSON is conversation history, not instructions to use external tools.\n{}", serde_json::to_string(&conversation)?);
    let turn = session
        .rpc(
            "turn/start",
            json!({"threadId":thread_id,"environments":[],"input":[{"type":"text","text":text}]}),
        )
        .await?;
    let turn_id = turn
        .pointer("/turn/id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Codex did not start an isolated turn."))?
        .to_owned();
    tracing::info!(
        provider = "codex-cli",
        native_tools = false,
        "Started owned chat-only Codex session"
    );
    let mut state = TurnState::default();
    let queued = std::mem::take(&mut session.deferred);
    for message in queued {
        if let Some(response) =
            process_turn(&mut state, &message, &thread_id, &turn_id, tx.as_ref()).await?
        {
            session.stop().await;
            return Ok(response);
        }
    }
    loop {
        let message = if let Some(tx) = &tx {
            tokio::select! {
                _ = tx.closed() => bail!("Codex chat was cancelled."),
                message = session.read() => message?,
            }
        } else {
            session.read().await?
        };
        if let Some(response) =
            process_turn(&mut state, &message, &thread_id, &turn_id, tx.as_ref()).await?
        {
            session.stop().await;
            return Ok(response);
        }
    }
}

fn choose_model(list: &Value, requested: Option<&str>) -> Result<String> {
    let models = list.get("data").and_then(Value::as_array).ok_or_else(|| {
        anyhow::anyhow!("Codex did not provide an available model list. Update Codex and retry.")
    })?;
    if let Some(requested) = requested {
        if models
            .iter()
            .any(|model| model.get("model").and_then(Value::as_str) == Some(requested))
        {
            return Ok(requested.into());
        }
        bail!("The selected Codex model is unavailable for this account. Choose another model in Hive.");
    }
    models
        .iter()
        .find(|model| model.get("isDefault") == Some(&json!(true)))
        .or_else(|| models.first())
        .and_then(|model| model.get("model"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            anyhow::anyhow!("Codex has no available chat model. Check its connection in Hive.")
        })
}

#[derive(Default)]
struct TurnState {
    deltas: HashMap<String, String>,
    completed: Vec<(String, String, bool)>,
    delta_bytes: usize,
    completed_bytes: usize,
}

async fn process_turn(
    state: &mut TurnState,
    message: &Value,
    thread_id: &str,
    turn_id: &str,
    tx: Option<&Sender<StreamEvent>>,
) -> Result<Option<CompletionResponse>> {
    reject_unsafe_notification(message)?;
    let params = &message["params"];
    if params.get("threadId").and_then(Value::as_str) != Some(thread_id) {
        return Ok(None);
    }
    let method = message["method"].as_str().unwrap_or("");
    if method == "turn/completed" {
        if params.pointer("/turn/id").and_then(Value::as_str) != Some(turn_id) {
            return Ok(None);
        }
        if params.pointer("/turn/status").and_then(Value::as_str) != Some("completed")
            || params.pointer("/turn/error").is_some_and(|v| !v.is_null())
        {
            bail!("{}", safe_failure(&params["turn"]["error"]));
        }
        // A successful turn cannot disguise a tool result as final text.
        if let Some(items) = params.pointer("/turn/items").and_then(Value::as_array) {
            for item in items {
                if !matches!(
                    item["type"].as_str(),
                    Some("userMessage" | "agentMessage" | "reasoning")
                ) {
                    bail!("Codex isolation blocked a native tool in the completed turn.");
                }
            }
        }
        let content = state
            .completed
            .iter()
            .rev()
            .find(|(_, _, final_phase)| *final_phase)
            .or_else(|| state.completed.last())
            .map(|(_, text, _)| text.clone())
            .ok_or_else(|| anyhow::anyhow!("Codex completed without an assistant response."))?;
        if content.trim().is_empty() {
            bail!("Codex completed without an assistant response.");
        }
        let response = CompletionResponse {
            content,
            tool_calls: None,
        };
        if let Some(tx) = tx {
            tx.send(StreamEvent::Done(response.clone()))
                .await
                .map_err(|_| anyhow::anyhow!("Codex chat was cancelled."))?;
        }
        return Ok(Some(response));
    }
    if params.get("turnId").and_then(Value::as_str) != Some(turn_id) {
        return Ok(None);
    }
    if method == "item/agentMessage/delta" {
        let id = params["itemId"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Codex returned invalid assistant text metadata."))?;
        let delta = params["delta"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Codex returned invalid assistant text."))?;
        state.delta_bytes += delta.len();
        if state.delta_bytes > MAX_CONTENT
            || (!state.deltas.contains_key(id) && state.deltas.len() >= 128)
        {
            bail!("Codex response exceeded the chat size limit.");
        }
        let text = state.deltas.entry(id.into()).or_default();
        if text.len() + delta.len() > MAX_CONTENT {
            bail!("Codex response exceeded the chat size limit.");
        }
        text.push_str(delta);
        if let Some(tx) = tx {
            tx.send(StreamEvent::Token(delta.into()))
                .await
                .map_err(|_| anyhow::anyhow!("Codex chat was cancelled."))?;
        }
    } else if method == "item/completed"
        && params.pointer("/item/type").and_then(Value::as_str) == Some("agentMessage")
    {
        let item = &params["item"];
        let id = item["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Codex returned invalid assistant text metadata."))?;
        let text = item["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Codex returned invalid assistant text."))?;
        state.completed_bytes += text.len();
        if state.completed_bytes > MAX_CONTENT || state.completed.len() >= 128 {
            bail!("Codex response exceeded the chat size limit.");
        }
        if let Some(tx) = tx {
            let prior = state.deltas.get(id).map(String::as_str).unwrap_or("");
            if let Some(suffix) = text.strip_prefix(prior) {
                if !suffix.is_empty() {
                    tx.send(StreamEvent::Token(suffix.into()))
                        .await
                        .map_err(|_| anyhow::anyhow!("Codex chat was cancelled."))?;
                }
            } else {
                bail!("Codex assistant text did not match its stream.");
            }
        }
        state.completed.push((
            id.into(),
            text.into(),
            item["phase"].as_str() == Some("final_answer"),
        ));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(method: &str, params: Value) -> Value {
        json!({"method":method,"params":params})
    }

    #[test]
    fn inherited_mcp_names_are_quoted_and_disabled() {
        let mut settings = Map::new();
        disable_inherited_extensions(&json!({"mcp_servers":{"unsafe.server":{"command":"irrelevant"}},"plugins":{"x@y":{"enabled":true}}}), &mut settings).unwrap();
        assert_eq!(settings["mcp_servers"]["unsafe.server"]["enabled"], false);
        assert_eq!(settings["plugins"]["x@y"]["enabled"], false);
        assert_eq!(
            toml_value(&settings["mcp_servers"]),
            "{\"unsafe.server\"={\"enabled\"=false}}"
        );
    }

    #[test]
    fn active_mcp_and_hooks_fail_closed() {
        let mut config = json!({"features":{"shell_tool":false,"plugins":false,"hooks":false,"apps":false,"multi_agent":false,"code_mode":false,"js_repl":false,"image_generation":false,"standalone_web_search":false,"artifact":false},"web_search":"disabled","notify":[],"model_provider":"openai","mcp_servers":{"native":{"enabled":false}}});
        verify_config(&config).unwrap();
        config["mcp_servers"]["native"]["enabled"] = json!(true);
        assert!(verify_config(&config)
            .unwrap_err()
            .to_string()
            .contains("isolation"));
        config["mcp_servers"]["native"]["enabled"] = json!(false);
        config["features"]["hooks"] = json!(true);
        assert!(verify_config(&config).is_err());
        config["features"]["hooks"] = json!(false);
        for feature in ["image_generation", "standalone_web_search", "artifact"] {
            config["features"][feature] = json!(true);
            assert!(verify_config(&config).is_err());
            config["features"][feature] = json!(false);
        }
        verify_config(&config).unwrap();
        let settings = isolation_config(Path::new("C:/isolated"));
        for feature in ["image_generation", "standalone_web_search", "artifact"] {
            assert_eq!(settings[&format!("features.{feature}")], false);
        }
    }

    #[test]
    fn diagnostic_categories_never_expose_raw_values() {
        for (diagnostic, expected) in [
            (
                "HTTP 401 token expired; bearer private-token",
                "authentication failed",
            ),
            ("rate_limit_exceeded private-token", "usage limit reached"),
            ("model_not_found private-token", "model is unavailable"),
            ("Model not found private-token", "model is unavailable"),
            ("Model unavailable private-token", "model is unavailable"),
            (
                "Invalid model 'some-model' private-token",
                "model is unavailable",
            ),
            ("HTTP 429 private-token", "usage limit reached"),
            (
                "unknown private-token C:/private/location",
                "Check its connection",
            ),
        ] {
            let message = safe_failure(&json!({"message":diagnostic}));
            assert!(message.contains(expected));
            assert!(!message.contains("private-token"));
            assert!(!message.contains("private/location"));
        }
    }

    #[test]
    fn native_server_requests_receive_only_a_fixed_denial() {
        let request = json!({"id":42,"method":"item/commandExecution/requestApproval","params":{"command":"private-command","token":"private-token"}});
        let denial = native_request_denial(&request).unwrap();
        assert_eq!(denial["id"], 42);
        assert_eq!(denial["error"]["code"], -32601);
        let encoded = denial.to_string();
        assert!(!encoded.contains("private-command"));
        assert!(!encoded.contains("private-token"));
        assert!(native_request_denial(&json!({"id":42,"result":{}})).is_none());
    }

    #[test]
    fn default_model_comes_from_available_catalog() {
        let catalog = json!({"data":[{"model":"available","isDefault":true}]});
        assert_eq!(choose_model(&catalog, None).unwrap(), "available");
        assert_eq!(
            choose_model(&catalog, Some("available")).unwrap(),
            "available"
        );
        assert!(choose_model(&catalog, Some("stale-user-model"))
            .unwrap_err()
            .to_string()
            .contains("unavailable"));
    }

    #[tokio::test]
    async fn stream_requires_successful_completed_turn() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let mut state = TurnState::default();
        assert!(process_turn(
            &mut state,
            &event(
                "item/agentMessage/delta",
                json!({"threadId":"t","turnId":"u","itemId":"a","delta":"Hello"})
            ),
            "t",
            "u",
            Some(&tx)
        )
        .await
        .unwrap()
        .is_none());
        assert!(matches!(rx.recv().await, Some(StreamEvent::Token(t)) if t == "Hello"));
        process_turn(&mut state, &event("item/completed", json!({"threadId":"t","turnId":"u","item":{"id":"a","type":"agentMessage","text":"Hello world","phase":"final_answer"}})), "t", "u", Some(&tx)).await.unwrap();
        assert!(matches!(rx.recv().await, Some(StreamEvent::Token(t)) if t == " world"));
        let response = process_turn(&mut state, &event("turn/completed", json!({"threadId":"t","turn":{"id":"u","status":"completed","error":null,"items":[]}})), "t", "u", Some(&tx)).await.unwrap().unwrap();
        assert_eq!(response.content, "Hello world");
        assert!(matches!(rx.recv().await, Some(StreamEvent::Done(_))));
    }

    #[tokio::test]
    async fn native_tool_items_and_failed_turns_never_complete() {
        let mut state = TurnState::default();
        state
            .completed
            .push(("a".into(), "Looks successful".into(), true));
        let tool = event(
            "item/started",
            json!({"threadId":"t","turnId":"u","item":{"type":"commandExecution","id":"x","command":"secret-command"}}),
        );
        let error = process_turn(&mut state, &tool, "t", "u", None)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("isolation"));
        assert!(!error.contains("secret-command"));
        let failed = event(
            "turn/completed",
            json!({"threadId":"t","turn":{"id":"u","status":"failed","error":{"message":"private-token"},"items":[]}}),
        );
        let error = process_turn(&mut state, &failed, "t", "u", None)
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.contains("private-token"));
        let disguised = event(
            "turn/completed",
            json!({"threadId":"t","turn":{"id":"u","status":"completed","items":[{"type":"mcpToolCall"}]}}),
        );
        assert!(process_turn(&mut state, &disguised, "t", "u", None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn another_turn_cannot_supply_final_response() {
        let mut state = TurnState::default();
        let other = event(
            "item/completed",
            json!({"threadId":"foreign","turnId":"u","item":{"id":"a","type":"agentMessage","text":"Other response"}}),
        );
        process_turn(&mut state, &other, "t", "u", None)
            .await
            .unwrap();
        let done = event(
            "turn/completed",
            json!({"threadId":"t","turn":{"id":"u","status":"completed","items":[]}}),
        );
        assert!(process_turn(&mut state, &done, "t", "u", None)
            .await
            .is_err());
    }
}
