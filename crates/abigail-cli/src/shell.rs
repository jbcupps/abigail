use abigail_birth::stages::BirthError;
use abigail_birth::BirthOrchestrator;
use abigail_capabilities::cognitive::LocalHttpProvider;
use abigail_core::ops::store_vault_secret;
use abigail_core::{templates, AppConfig, SecretsVault};
use anyhow::{anyhow, Context};
use chrono::Utc;
use entity_core::{
    ApiEnvelope as EntityApiEnvelope, ChatRequest, ChatResponse, DEFAULT_ENTITY_ADDR,
    ENTITY_API_VERSION_PREFIX,
};
use hive_core::{
    ApiEnvelope as HiveApiEnvelope, BirthEntityRequest, BirthPath, EntityRecord,
    StartStopEntityRequest, DEFAULT_HIVE_ADDR, HIVE_API_VERSION_PREFIX,
};
use reqwest::Client;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::{sleep, Instant};

const PROVIDER_CHOICES: &str =
    "openai, anthropic, perplexity, xai, google, tavily, codex, claude, gemini, grok";

struct ShellState {
    client: Client,
    config: AppConfig,
    vault: SecretsVault,
    hive_base: String,
    entity_base: String,
    workspace_root: Option<PathBuf>,
    provider_config_changed: bool,
    local_provider_found: bool,
}

#[derive(Debug, Clone)]
struct DaemonStatus {
    was_running: bool,
}

#[derive(Debug, Clone)]
struct LocalProvider {
    url: String,
    label: &'static str,
}

pub async fn run_shell() -> anyhow::Result<()> {
    println!("abigail shell: initializing hive/entity session");

    let client = Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .context("failed to build HTTP client")?;
    let workspace_root = find_workspace_root();
    let config = load_config()?;
    let vault = load_vault(&config)?;

    let hive_base = format!("http://{}{}", DEFAULT_HIVE_ADDR, HIVE_API_VERSION_PREFIX);
    let entity_base = format!(
        "http://{}{}",
        DEFAULT_ENTITY_ADDR, ENTITY_API_VERSION_PREFIX
    );

    let mut state = ShellState {
        client,
        config: config.clone(),
        vault,
        hive_base,
        entity_base,
        workspace_root,
        provider_config_changed: false,
        local_provider_found: false,
    };

    ensure_hive_daemon(&state).await?;

    if let Some(local) = detect_local_provider(&mut state).await? {
        println!("local provider detected: {} ({})", local.label, local.url);
        state.local_provider_found = true;
    } else {
        println!(
            "no local provider detected at LM Studio/Ollama defaults; cloud key onboarding required"
        );
    }

    if !state.local_provider_found && !has_cloud_provider_key(&state.vault) {
        println!("provide at least one cloud provider key to continue");
        prompt_provider_setup(&mut state, true)?;
    } else if !has_cloud_provider_key(&state.vault) {
        let add_cloud =
            prompt_yes_no("no cloud key found. add one now for stronger ego routing? [y/N]: ")?;
        if add_cloud {
            prompt_provider_setup(&mut state, false)?;
        }
    }

    run_simple_birth_cycle(&mut state)?;

    let entity_default = slugify(state.config.agent_name.as_deref().unwrap_or("adam").trim());
    let entity_id = prompt_with_default("entity id", &entity_default)?;

    let entity_daemon = ensure_entity_daemon(&state).await?;
    if entity_daemon.was_running && state.provider_config_changed {
        println!(
            "entity-daemon was already running before provider updates; restart it to fully apply new provider settings"
        );
    }

    register_and_start_entity(&state, &entity_id).await?;
    run_entity_chat_loop(&state, &entity_id).await?;
    Ok(())
}

fn load_config() -> anyhow::Result<AppConfig> {
    let mut defaults = AppConfig::default_paths();
    if let Some(override_data_dir) = configured_data_dir_override() {
        defaults.data_dir = override_data_dir.clone();
        defaults.models_dir = override_data_dir.join("models");
        defaults.docs_dir = override_data_dir.join("docs");
        defaults.db_path = override_data_dir.join("abigail_seed.db");
        defaults.external_pubkey_path = None;
    }
    let config_path = defaults.config_path();
    if config_path.exists() {
        let mut loaded = AppConfig::load(&config_path)
            .with_context(|| format!("failed to load config at {}", config_path.display()))?;
        if loaded.data_dir.as_os_str().is_empty() {
            loaded.data_dir = defaults.data_dir;
        }
        Ok(loaded)
    } else {
        defaults
            .save(&config_path)
            .with_context(|| format!("failed to initialize config at {}", config_path.display()))?;
        Ok(defaults)
    }
}

fn load_vault(config: &AppConfig) -> anyhow::Result<SecretsVault> {
    SecretsVault::load(config.data_dir.clone()).map_err(|e| anyhow!(e.to_string()))
}

fn configured_data_dir_override() -> Option<PathBuf> {
    let raw = std::env::var("ABIGAIL_DATA_DIR").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

async fn ensure_hive_daemon(state: &ShellState) -> anyhow::Result<DaemonStatus> {
    ensure_daemon_running(
        &state.client,
        "hive-daemon",
        "hive-daemon",
        &format!("{}/status", state.hive_base),
        state.workspace_root.as_deref(),
    )
    .await
}

async fn ensure_entity_daemon(state: &ShellState) -> anyhow::Result<DaemonStatus> {
    ensure_daemon_running(
        &state.client,
        "entity-daemon",
        "entity-daemon",
        &format!("{}/status", state.entity_base),
        state.workspace_root.as_deref(),
    )
    .await
}

async fn ensure_daemon_running(
    client: &Client,
    daemon_name: &str,
    package_name: &str,
    status_url: &str,
    workspace_root: Option<&Path>,
) -> anyhow::Result<DaemonStatus> {
    if endpoint_ready(client, status_url).await {
        println!("{} already running", daemon_name);
        return Ok(DaemonStatus { was_running: true });
    }

    let (stdout_path, stderr_path) = daemon_log_paths(daemon_name, workspace_root)?;
    spawn_daemon(package_name, workspace_root, &stdout_path, &stderr_path)?;

    let timeout = Duration::from_secs(180);
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if endpoint_ready(client, status_url).await {
            println!(
                "{} started (logs: {}, {})",
                daemon_name,
                stdout_path.display(),
                stderr_path.display()
            );
            return Ok(DaemonStatus { was_running: false });
        }
        sleep(Duration::from_millis(500)).await;
    }

    Err(anyhow!(
        "{} did not become ready within {}s (see logs: {}, {})",
        daemon_name,
        timeout.as_secs(),
        stdout_path.display(),
        stderr_path.display()
    ))
}

fn daemon_log_paths(
    daemon_name: &str,
    workspace_root: Option<&Path>,
) -> anyhow::Result<(PathBuf, PathBuf)> {
    let base = workspace_root
        .map(|root| root.join("target").join("abigail-shell-logs"))
        .unwrap_or_else(|| std::env::temp_dir().join("abigail-shell-logs"));
    fs::create_dir_all(&base)
        .with_context(|| format!("failed to create log dir {}", base.display()))?;
    Ok((
        base.join(format!("{daemon_name}.out.log")),
        base.join(format!("{daemon_name}.err.log")),
    ))
}

fn spawn_daemon(
    package_name: &str,
    workspace_root: Option<&Path>,
    stdout_path: &Path,
    stderr_path: &Path,
) -> anyhow::Result<()> {
    let stdout_file = File::create(stdout_path)
        .with_context(|| format!("failed to create {}", stdout_path.display()))?;
    let stderr_file = File::create(stderr_path)
        .with_context(|| format!("failed to create {}", stderr_path.display()))?;

    let mut cmd = Command::new("cargo");
    cmd.arg("run").arg("-p").arg(package_name);
    if let Some(root) = workspace_root {
        cmd.current_dir(root);
    }
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::from(stdout_file));
    cmd.stderr(Stdio::from(stderr_file));
    cmd.spawn()
        .with_context(|| format!("failed to spawn cargo run -p {}", package_name))?;
    Ok(())
}

async fn endpoint_ready(client: &Client, url: &str) -> bool {
    match client.get(url).send().await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

async fn detect_local_provider(state: &mut ShellState) -> anyhow::Result<Option<LocalProvider>> {
    if env_flag_enabled("ABIGAIL_SHELL_SKIP_LOCAL_PROVIDER") {
        return Ok(None);
    }

    let mut candidates: Vec<String> = Vec::new();
    if let Some(existing) = state.config.local_llm_base_url.clone() {
        candidates.push(existing);
    }
    candidates.extend(
        [
            "http://127.0.0.1:1234",
            "http://localhost:1234",
            "http://127.0.0.1:11434",
            "http://localhost:11434",
        ]
        .iter()
        .map(|s| s.to_string()),
    );

    let mut seen = HashSet::new();
    for candidate in candidates {
        if !seen.insert(candidate.clone()) {
            continue;
        }

        let provider = match LocalHttpProvider::with_url_auto_model(candidate.clone()).await {
            Ok(provider) => provider,
            Err(_) => continue,
        };
        if provider.heartbeat().await.is_err() {
            continue;
        }

        if state.config.local_llm_base_url.as_deref() != Some(candidate.as_str()) {
            state.config.local_llm_base_url = Some(candidate.clone());
            let config_path = state.config.config_path();
            state
                .config
                .save(&config_path)
                .with_context(|| format!("failed to save config at {}", config_path.display()))?;
            state.provider_config_changed = true;
        }

        return Ok(Some(LocalProvider {
            label: local_provider_label(&candidate),
            url: candidate,
        }));
    }
    Ok(None)
}

fn local_provider_label(url: &str) -> &'static str {
    if url.contains(":11434") {
        "Ollama"
    } else if url.contains(":1234") {
        "LM Studio"
    } else {
        "Local OpenAI-compatible provider"
    }
}

fn has_cloud_provider_key(vault: &SecretsVault) -> bool {
    [
        "openai",
        "anthropic",
        "perplexity",
        "xai",
        "google",
        "claude-cli",
        "gemini-cli",
        "codex-cli",
        "grok-cli",
    ]
    .iter()
    .any(|provider| {
        vault
            .get_secret(provider)
            .is_some_and(|value| !value.trim().is_empty())
    })
}

fn prompt_provider_setup(state: &mut ShellState, require_one: bool) -> anyhow::Result<()> {
    let mut added = 0usize;
    loop {
        let prompt = if require_one && added == 0 {
            format!(
                "provider ({}), or paste API key directly: ",
                PROVIDER_CHOICES
            )
        } else {
            format!("provider ({}), key, or 'done': ", PROVIDER_CHOICES)
        };

        let input = prompt_line(&prompt)?;
        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        if matches_ignore_case(input, &["done", "skip", "none"]) {
            if require_one && added == 0 {
                println!("at least one provider key is required in this path");
                continue;
            }
            break;
        }

        if let Some(detected_provider) = detect_provider_from_key(input) {
            store_provider_alias(state, detected_provider, input)?;
            added += 1;
        } else {
            let Some(provider) = normalize_provider_alias(input) else {
                println!(
                    "unknown provider '{}'. expected one of: {}",
                    input, PROVIDER_CHOICES
                );
                continue;
            };

            let key = prompt_line(&format!("paste {} API key: ", provider))?;
            let key = key.trim();
            if key.is_empty() {
                println!("key cannot be empty");
                continue;
            }
            store_provider_alias(state, provider, key)?;
            added += 1;
        }

        let add_another = prompt_yes_no("add another provider key? [y/N]: ")?;
        if !add_another {
            break;
        }
    }

    Ok(())
}

fn store_provider_alias(
    state: &mut ShellState,
    canonical_provider: &str,
    key: &str,
) -> anyhow::Result<()> {
    let targets = provider_secret_targets(canonical_provider);
    for target in &targets {
        store_vault_secret(&mut state.vault, target, key)
            .map_err(|e| anyhow!("failed storing {} key: {}", target, e))?;
    }

    if canonical_provider == "openai" {
        state.config.openai_api_key = Some(key.to_string());
    }
    if canonical_provider != "tavily" {
        state.config.active_provider_preference = Some(canonical_provider.to_string());
    }

    let config_path = state.config.config_path();
    state
        .config
        .save(&config_path)
        .with_context(|| format!("failed to save config at {}", config_path.display()))?;
    state.provider_config_changed = true;

    println!(
        "stored key for {} (aliases: {})",
        canonical_provider,
        targets.join(", ")
    );
    Ok(())
}

fn provider_secret_targets(canonical_provider: &str) -> Vec<String> {
    match canonical_provider {
        "openai" => vec!["openai".to_string(), "codex-cli".to_string()],
        "anthropic" => vec!["anthropic".to_string(), "claude-cli".to_string()],
        "google" => vec!["google".to_string(), "gemini-cli".to_string()],
        "xai" => vec!["xai".to_string(), "grok-cli".to_string()],
        "perplexity" => vec!["perplexity".to_string()],
        "tavily" => vec!["tavily".to_string()],
        _ => vec![canonical_provider.to_string()],
    }
}

fn normalize_provider_alias(value: &str) -> Option<&'static str> {
    match value.trim().to_lowercase().as_str() {
        "openai" | "oai" | "chatgpt" | "gpt" | "codex" | "codex-cli" => Some("openai"),
        "anthropic" | "claude" | "claude-cli" => Some("anthropic"),
        "google" | "gemini" | "gemini-cli" => Some("google"),
        "xai" | "grok" | "grok-cli" => Some("xai"),
        "perplexity" | "pplx" => Some("perplexity"),
        "tavily" | "search" => Some("tavily"),
        _ => None,
    }
}

fn detect_provider_from_key(value: &str) -> Option<&'static str> {
    let key = value.trim();
    if key.starts_with("sk-ant-") {
        Some("anthropic")
    } else if key.starts_with("sk-") {
        Some("openai")
    } else if key.starts_with("pplx-") {
        Some("perplexity")
    } else if key.starts_with("xai-") {
        Some("xai")
    } else if key.starts_with("AIza") {
        Some("google")
    } else if key.starts_with("tvly-") {
        Some("tavily")
    } else {
        None
    }
}

fn run_simple_birth_cycle(state: &mut ShellState) -> anyhow::Result<()> {
    if state.config.birth_complete {
        let label = state
            .config
            .agent_name
            .clone()
            .unwrap_or_else(|| "agent".to_string());
        println!("birth already complete for {}", label);
        return Ok(());
    }

    println!("starting simple birth cycle");

    ensure_constitutional_docs(&state.config.docs_dir)?;
    let docs_dir = state.config.docs_dir.clone();
    let mut orchestrator = match BirthOrchestrator::new(state.config.clone()) {
        Ok(orchestrator) => orchestrator,
        Err(err) => {
            if err
                .downcast_ref::<BirthError>()
                .is_some_and(|e| matches!(e, BirthError::AlreadyBorn))
            {
                state.config.birth_complete = true;
                let config_path = state.config.config_path();
                state.config.save(&config_path).with_context(|| {
                    format!("failed to save config at {}", config_path.display())
                })?;
                println!("birth already recorded; continuing to chat");
                return Ok(());
            }
            return Err(err).context("failed to initialize birth orchestrator");
        }
    };

    orchestrator
        .generate_identity(&docs_dir)
        .context("darkness stage failed")?;
    if let Some(private_key) = orchestrator.get_private_key_base64() {
        println!();
        println!("darkness stage complete");
        println!("save this private birth key now (it will not be shown again):");
        println!("{}", private_key);
        println!();
    }
    let _ = prompt_line("press Enter after you have saved the private key: ")?;

    orchestrator
        .advance_past_darkness()
        .context("failed to advance past darkness")?;
    orchestrator
        .advance_to_connectivity()
        .context("failed to advance to connectivity")?;

    if !state.local_provider_found && !has_cloud_provider_key(&state.vault) {
        return Err(anyhow!(
            "connectivity stage requires a local provider or at least one cloud provider key"
        ));
    }

    orchestrator
        .advance_to_crystallization()
        .context("failed to advance to crystallization")?;

    let mentor_default = std::env::var("USERNAME").unwrap_or_else(|_| "my mentor".to_string());
    let mentor_name = prompt_with_default("mentor name", &mentor_default)?;
    let default_name = state
        .config
        .agent_name
        .clone()
        .unwrap_or_else(|| "Abigail".to_string());
    let agent_name = prompt_with_default("entity name", &default_name)?;
    let purpose = prompt_with_default(
        "entity purpose",
        "Help my mentor build, reason, and execute safely.",
    )?;
    let personality = prompt_with_default("personality/tone", "Warm, direct, and pragmatic.")?;

    let soul = templates::fill_soul_template(&agent_name, &purpose, &personality, &mentor_name);
    let growth = templates::GROWTH_MD.to_string();

    {
        let cfg = orchestrator.config_mut();
        cfg.agent_name = Some(agent_name.clone());
        if cfg.birth_timestamp.is_none() {
            cfg.birth_timestamp = Some(Utc::now().to_rfc3339());
        }
    }

    orchestrator
        .crystallize_soul(&soul, &growth)
        .context("failed to crystallize soul")?;
    orchestrator
        .complete_emergence()
        .context("failed to complete emergence")?;

    state.config = orchestrator.config().clone();
    println!("birth complete: {} is now ready", agent_name);
    Ok(())
}

fn ensure_constitutional_docs(docs_dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(docs_dir)
        .with_context(|| format!("failed to create docs dir {}", docs_dir.display()))?;
    for (name, content) in templates::CONSTITUTIONAL_DOCS {
        let path = docs_dir.join(name);
        if !path.exists() {
            fs::write(&path, content)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
    }
    Ok(())
}

async fn register_and_start_entity(state: &ShellState, entity_id: &str) -> anyhow::Result<()> {
    let birth = state
        .client
        .post(format!("{}/entity/birth", state.hive_base))
        .json(&BirthEntityRequest {
            id: entity_id.to_string(),
            path: BirthPath::QuickStart,
        })
        .send()
        .await
        .context("failed to call hive birth endpoint")?
        .error_for_status()
        .context("hive birth endpoint returned error")?
        .json::<HiveApiEnvelope<EntityRecord>>()
        .await
        .context("failed parsing hive birth response")?;

    let started = state
        .client
        .post(format!("{}/entity/start", state.hive_base))
        .json(&StartStopEntityRequest {
            id: entity_id.to_string(),
        })
        .send()
        .await
        .context("failed to call hive start endpoint")?
        .error_for_status()
        .context("hive start endpoint returned error")?
        .json::<HiveApiEnvelope<EntityRecord>>()
        .await
        .context("failed parsing hive start response")?;

    println!(
        "entity '{}' ready (birth_path={:?}, status={:?})",
        birth.data.id, birth.data.birth_path, started.data.status
    );
    Ok(())
}

async fn run_entity_chat_loop(state: &ShellState, entity_id: &str) -> anyhow::Result<()> {
    println!();
    println!(
        "chat connected to entity '{}'. type 'exit' or '/exit' to leave.",
        entity_id
    );
    println!();

    loop {
        let message = prompt_line("mentor> ")?;
        let message = message.trim();
        if message.is_empty() {
            continue;
        }
        if matches_ignore_case(message, &["exit", "/exit", "quit", "/quit"]) {
            println!("chat ended");
            break;
        }

        let response = state
            .client
            .post(format!("{}/chat", state.entity_base))
            .json(&ChatRequest {
                message: message.to_string(),
            })
            .send()
            .await;

        match response {
            Ok(resp) => match resp.error_for_status() {
                Ok(ok) => match ok.json::<EntityApiEnvelope<ChatResponse>>().await {
                    Ok(payload) => println!("entity> {}", payload.data.reply),
                    Err(err) => println!("entity> failed parsing response: {}", err),
                },
                Err(err) => println!("entity> request failed: {}", err),
            },
            Err(err) => println!("entity> transport error: {}", err),
        }
    }

    Ok(())
}

fn prompt_yes_no(prompt: &str) -> anyhow::Result<bool> {
    let answer = prompt_line(prompt)?;
    Ok(matches_ignore_case(answer.trim(), &["y", "yes"]))
}

fn prompt_with_default(label: &str, default: &str) -> anyhow::Result<String> {
    let answer = prompt_line(&format!("{} [{}]: ", label, default))?;
    let trimmed = answer.trim();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn prompt_line(prompt: &str) -> anyhow::Result<String> {
    print!("{}", prompt);
    io::stdout().flush().context("failed to flush stdout")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read stdin")?;
    Ok(input)
}

fn matches_ignore_case(value: &str, options: &[&str]) -> bool {
    options.iter().any(|opt| value.eq_ignore_ascii_case(opt))
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "adam".to_string()
    } else {
        trimmed.to_string()
    }
}

fn find_workspace_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("Cargo.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_provider_alias_supports_cli_names() {
        assert_eq!(normalize_provider_alias("codex"), Some("openai"));
        assert_eq!(normalize_provider_alias("claude"), Some("anthropic"));
        assert_eq!(normalize_provider_alias("gemini"), Some("google"));
        assert_eq!(normalize_provider_alias("grok"), Some("xai"));
        assert_eq!(normalize_provider_alias("perplexity"), Some("perplexity"));
    }

    #[test]
    fn detect_provider_from_key_prefixes() {
        assert_eq!(detect_provider_from_key("sk-ant-abc123"), Some("anthropic"));
        assert_eq!(detect_provider_from_key("sk-abc123"), Some("openai"));
        assert_eq!(detect_provider_from_key("pplx-abc123"), Some("perplexity"));
        assert_eq!(detect_provider_from_key("xai-abc123"), Some("xai"));
        assert_eq!(detect_provider_from_key("AIzaabc123"), Some("google"));
        assert_eq!(detect_provider_from_key("tvly-abc123"), Some("tavily"));
        assert_eq!(detect_provider_from_key("not-a-key"), None);
    }

    #[test]
    fn slugify_generates_safe_entity_id() {
        assert_eq!(slugify("Adam Prime"), "adam-prime");
        assert_eq!(slugify("  !! "), "adam");
        assert_eq!(slugify("A_B.C"), "a-b-c");
    }
}
