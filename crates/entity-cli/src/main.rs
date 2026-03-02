//! Entity CLI — thin reqwest client for entity-daemon.
//!
//! Usage:
//!   entity-cli status
//!   entity-cli chat "hello"
//!   entity-cli skills
//!   entity-cli tool <skill_id> <tool_name> [params_json]
//!   entity-cli memory recent
//!   entity-cli memory search "keyword"
//!   entity-cli scaffold my-skill --type dynamic

use clap::{Parser, Subcommand, ValueEnum};
use entity_core::{
    ApiEnvelope, CancelJobResponse, ChatRequest, ChatResponse, EntityStatus, JobStatusResponse,
    ListJobsResponse, MemoryEntry, MemorySearchRequest, MemoryStats, SkillInfo, SubmitJobRequest,
    SubmitJobResponse, ToolExecRequest, ToolExecResponse, TopicResultsResponse,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "entity-cli", about = "CLI client for Abigail Entity daemon")]
struct Cli {
    /// Entity daemon URL
    #[arg(long, default_value = "http://127.0.0.1:3142", global = true)]
    url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, ValueEnum)]
enum SkillType {
    /// JSON-config HTTP skill (no Rust code needed)
    Dynamic,
    /// Rust native skill with Cargo.toml
    Native,
}

#[derive(Subcommand)]
enum Commands {
    /// Show entity status
    Status,
    /// Send a chat message
    Chat {
        /// The message to send
        message: String,
        /// Optional target: ID or EGO
        #[arg(long)]
        target: Option<String>,
    },
    /// List loaded skills
    Skills,
    /// Execute a tool
    Tool {
        /// Skill ID (e.g., "builtin.hive_management")
        skill_id: String,
        /// Tool name (e.g., "list_entities")
        tool_name: String,
        /// JSON parameters (optional)
        params: Option<String>,
    },
    /// Query or manage persistent memories
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
    /// Diagnose routing decision for a test message (no LLM call)
    Diagnose {
        /// Test message to diagnose routing for
        #[arg(default_value = "hello")]
        message: String,
    },
    /// Manage queued async jobs
    Jobs {
        #[command(subcommand)]
        action: JobAction,
    },
    /// Topic-level queue operations
    Topics {
        #[command(subcommand)]
        action: TopicAction,
    },
    /// Scaffold a new skill directory with template files
    Scaffold {
        /// Skill name (e.g., "my-weather-api")
        name: String,
        /// Skill type: dynamic (JSON config) or native (Rust)
        #[arg(long, value_enum, default_value = "dynamic")]
        r#type: SkillType,
        /// Output directory (defaults to ./skills/)
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Sign a skill allowlist entry (produces JSON for config)
    SkillSign {
        /// Skill ID to sign (e.g., "dynamic.github_api")
        #[arg(long)]
        skill_id: String,
        /// Signer identifier (base64-encoded Ed25519 public key)
        #[arg(long)]
        signer: String,
        /// Source label (e.g., "operator", "ci-pipeline")
        #[arg(long)]
        source: String,
        /// Path to Ed25519 private key file (raw 32 bytes or 64-byte keypair)
        #[arg(long)]
        private_key: PathBuf,
    },
}

#[derive(Subcommand)]
enum MemoryAction {
    /// Show memory statistics
    Stats,
    /// Show recent memories
    Recent {
        /// Max number of memories to return
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Search memories by keyword
    Search {
        /// Search query
        query: String,
        /// Max number of results
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// Insert a new memory
    Insert {
        /// Memory content
        content: String,
        /// Weight tier: ephemeral, distilled, crystallized
        #[arg(long, default_value = "ephemeral")]
        weight: String,
    },
}

#[derive(Subcommand)]
enum JobAction {
    /// Submit a new queued job
    Submit {
        goal: String,
        #[arg(long)]
        topic: String,
        #[arg(long)]
        capability: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        time_budget_ms: Option<u64>,
        #[arg(long)]
        max_turns: Option<u32>,
        #[arg(long)]
        ttl_seconds: Option<u64>,
    },
    /// Get status for a specific job
    Status { job_id: String },
    /// List jobs, optionally filtered by status
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value = "50")]
        limit: usize,
    },
    /// Cancel a queued/running job
    Cancel { job_id: String },
}

#[derive(Subcommand)]
enum TopicAction {
    /// Get completed results for a topic
    Results {
        topic: String,
        #[arg(long, default_value = "50")]
        limit: usize,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();
    let base = cli.url.trim_end_matches('/');

    match cli.command {
        Commands::Status => {
            let resp: ApiEnvelope<EntityStatus> = client
                .get(format!("{}/v1/status", base))
                .send()
                .await?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Commands::Chat { message, target } => {
            let resp: ApiEnvelope<ChatResponse> = client
                .post(format!("{}/v1/chat", base))
                .json(&ChatRequest {
                    message,
                    target,
                    session_messages: None,
                    session_id: None,
                })
                .send()
                .await?
                .json()
                .await?;
            if let Some(data) = &resp.data {
                println!("{}", data.reply);
            } else if let Some(err) = &resp.error {
                eprintln!("Error: {}", err);
            }
        }
        Commands::Diagnose { message } => {
            let url = reqwest::Url::parse_with_params(
                &format!("{}/v1/routing/diagnose", base),
                &[("message", message.as_str())],
            )?;
            let resp: ApiEnvelope<serde_json::Value> = client.get(url).send().await?.json().await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Commands::Jobs { action } => match action {
            JobAction::Submit {
                goal,
                topic,
                capability,
                priority,
                time_budget_ms,
                max_turns,
                ttl_seconds,
            } => {
                let resp: ApiEnvelope<SubmitJobResponse> = client
                    .post(format!("{}/v1/jobs/submit", base))
                    .json(&SubmitJobRequest {
                        goal,
                        topic,
                        capability,
                        priority,
                        time_budget_ms,
                        max_turns,
                        system_context: None,
                        allowed_skill_ids: None,
                        ttl_seconds,
                        input_data: None,
                        parent_job_id: None,
                    })
                    .send()
                    .await?
                    .json()
                    .await?;
                println!("{}", serde_json::to_string_pretty(&resp)?);
            }
            JobAction::Status { job_id } => {
                let resp: ApiEnvelope<JobStatusResponse> = client
                    .get(format!("{}/v1/jobs/{}", base, job_id))
                    .send()
                    .await?
                    .json()
                    .await?;
                println!("{}", serde_json::to_string_pretty(&resp)?);
            }
            JobAction::List { status, limit } => {
                let mut url = format!("{}/v1/jobs?limit={}", base, limit);
                if let Some(status) = status {
                    url.push_str(&format!("&status={}", status));
                }
                let resp: ApiEnvelope<ListJobsResponse> =
                    client.get(url).send().await?.json().await?;
                println!("{}", serde_json::to_string_pretty(&resp)?);
            }
            JobAction::Cancel { job_id } => {
                let resp: ApiEnvelope<CancelJobResponse> = client
                    .post(format!("{}/v1/jobs/{}/cancel", base, job_id))
                    .send()
                    .await?
                    .json()
                    .await?;
                println!("{}", serde_json::to_string_pretty(&resp)?);
            }
        },
        Commands::Topics { action } => match action {
            TopicAction::Results { topic, limit } => {
                let resp: ApiEnvelope<TopicResultsResponse> = client
                    .get(format!(
                        "{}/v1/topics/{}/results?limit={}",
                        base, topic, limit
                    ))
                    .send()
                    .await?
                    .json()
                    .await?;
                println!("{}", serde_json::to_string_pretty(&resp)?);
            }
        },
        Commands::Skills => {
            let resp: ApiEnvelope<Vec<SkillInfo>> = client
                .get(format!("{}/v1/skills", base))
                .send()
                .await?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Commands::Tool {
            skill_id,
            tool_name,
            params,
        } => {
            let params_value = if let Some(p) = params {
                serde_json::from_str(&p)?
            } else {
                serde_json::json!({})
            };
            let resp: ApiEnvelope<ToolExecResponse> = client
                .post(format!("{}/v1/tools/execute", base))
                .json(&ToolExecRequest {
                    skill_id,
                    tool_name,
                    params: params_value,
                })
                .send()
                .await?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Commands::Memory { action } => match action {
            MemoryAction::Stats => {
                let resp: ApiEnvelope<MemoryStats> = client
                    .get(format!("{}/v1/memory/stats", base))
                    .send()
                    .await?
                    .json()
                    .await?;
                println!("{}", serde_json::to_string_pretty(&resp)?);
            }
            MemoryAction::Recent { limit } => {
                let resp: ApiEnvelope<Vec<MemoryEntry>> = client
                    .get(format!("{}/v1/memory/recent?limit={}", base, limit))
                    .send()
                    .await?
                    .json()
                    .await?;
                if let Some(entries) = &resp.data {
                    for entry in entries {
                        println!(
                            "[{}] ({}) {}: {}",
                            entry.created_at,
                            entry.weight,
                            entry.id.chars().take(8).collect::<String>(),
                            entry.content
                        );
                    }
                    if entries.is_empty() {
                        println!("No memories stored yet.");
                    }
                } else if let Some(err) = &resp.error {
                    eprintln!("Error: {}", err);
                }
            }
            MemoryAction::Search { query, limit } => {
                let resp: ApiEnvelope<Vec<MemoryEntry>> = client
                    .post(format!("{}/v1/memory/search", base))
                    .json(&MemorySearchRequest { query, limit })
                    .send()
                    .await?
                    .json()
                    .await?;
                if let Some(entries) = &resp.data {
                    for entry in entries {
                        println!(
                            "[{}] ({}) {}: {}",
                            entry.created_at,
                            entry.weight,
                            entry.id.chars().take(8).collect::<String>(),
                            entry.content
                        );
                    }
                    if entries.is_empty() {
                        println!("No matches found.");
                    }
                } else if let Some(err) = &resp.error {
                    eprintln!("Error: {}", err);
                }
            }
            MemoryAction::Insert { content, weight } => {
                let resp: ApiEnvelope<MemoryEntry> = client
                    .post(format!("{}/v1/memory/insert", base))
                    .json(&entity_core::MemoryInsertRequest { content, weight })
                    .send()
                    .await?
                    .json()
                    .await?;
                if let Some(entry) = &resp.data {
                    println!("Stored memory {} ({})", entry.id, entry.weight);
                } else if let Some(err) = &resp.error {
                    eprintln!("Error: {}", err);
                }
            }
        },
        Commands::Scaffold {
            name,
            r#type,
            output,
        } => {
            scaffold_skill(&name, &r#type, output.as_deref())?;
        }
        Commands::SkillSign {
            skill_id,
            signer,
            source,
            private_key,
        } => {
            sign_skill_allowlist_entry(&skill_id, &signer, &source, &private_key)?;
        }
    }

    Ok(())
}

fn sign_skill_allowlist_entry(
    skill_id: &str,
    signer: &str,
    source: &str,
    private_key_path: &std::path::Path,
) -> anyhow::Result<()> {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    use ed25519_dalek::{Signer, SigningKey};

    let key_bytes = std::fs::read(private_key_path)?;
    let signing_key = match key_bytes.len() {
        32 => {
            let bytes: [u8; 32] = key_bytes
                .try_into()
                .map_err(|_| anyhow::anyhow!("Invalid key length"))?;
            SigningKey::from_bytes(&bytes)
        }
        64 => {
            // Ed25519 keypair: first 32 bytes are the secret scalar
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(&key_bytes[..32]);
            SigningKey::from_bytes(&bytes)
        }
        other => {
            anyhow::bail!(
                "Expected 32-byte or 64-byte Ed25519 key file, got {} bytes",
                other
            );
        }
    };

    let payload = abigail_skills::build_allowlist_payload(skill_id, signer, source, true);
    let signature = signing_key.sign(payload.as_bytes());
    let sig_b64 = BASE64.encode(signature.to_bytes());

    let entry = serde_json::json!({
        "skill_id": skill_id,
        "signer": signer,
        "signature": sig_b64,
        "source": source,
        "added_at": chrono::Utc::now().to_rfc3339(),
        "active": true,
    });

    println!("{}", serde_json::to_string_pretty(&entry)?);

    // Verification sanity check
    let verifying_key = signing_key.verifying_key();
    let expected_signer = BASE64.encode(verifying_key.to_bytes());
    if expected_signer != signer {
        eprintln!("Warning: --signer does not match the public key derived from --private-key.");
        eprintln!("  Expected: {}", expected_signer);
        eprintln!("  Got:      {}", signer);
        eprintln!("The signature is valid for the private key but will fail verification");
        eprintln!("unless the matching public key is in trusted_skill_signers.");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Skill scaffolding
// ---------------------------------------------------------------------------

fn scaffold_skill(
    name: &str,
    skill_type: &SkillType,
    output: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let base_dir = output.unwrap_or_else(|| std::path::Path::new("skills"));
    let skill_dir = base_dir.join(format!("skill-{}", name));

    if skill_dir.exists() {
        anyhow::bail!("Directory already exists: {}", skill_dir.display());
    }

    std::fs::create_dir_all(&skill_dir)?;

    let skill_id = format!("custom.{}", name.replace('-', "_"));

    match skill_type {
        SkillType::Dynamic => scaffold_dynamic_skill(&skill_dir, name, &skill_id)?,
        SkillType::Native => scaffold_native_skill(&skill_dir, name, &skill_id)?,
    }

    println!(
        "Scaffolded {} skill: {}",
        match skill_type {
            SkillType::Dynamic => "dynamic",
            SkillType::Native => "native",
        },
        skill_dir.display()
    );

    Ok(())
}

fn scaffold_dynamic_skill(dir: &std::path::Path, name: &str, skill_id: &str) -> anyhow::Result<()> {
    // skill.toml manifest
    let toml_content = format!(
        r#"[skill]
id = "{skill_id}"
name = "{display_name}"
version = "0.1.0"
description = "TODO: describe what this skill does"
category = "General"

[runtime]
runtime = "DynamicApi"

[[permissions]]
permission = {{ Network = {{ Domains = ["api.example.com"] }} }}
reason = "API access"
optional = false
"#,
        skill_id = skill_id,
        display_name = to_title_case(name),
    );
    std::fs::write(dir.join("skill.toml"), toml_content)?;

    // Dynamic API config JSON
    let json_content = serde_json::to_string_pretty(&serde_json::json!({
        "id": skill_id,
        "name": to_title_case(name),
        "description": "TODO: describe what this skill does",
        "version": "0.1.0",
        "category": "General",
        "created_at": chrono_now_stub(),
        "tools": [
            {
                "name": "example_tool",
                "description": "TODO: describe this tool",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The input query"
                        }
                    },
                    "required": ["query"]
                },
                "method": "GET",
                "url_template": "https://api.example.com/v1/search?q={{query}}",
                "headers": {},
                "body_template": null,
                "response_extract": {
                    "result": "data.result"
                },
                "response_format": "Result: {{result}}"
            }
        ]
    }))?;
    std::fs::write(dir.join(format!("{}.json", skill_id)), json_content)?;

    println!("  Created skill.toml (manifest)");
    println!("  Created {}.json (API config)", skill_id);
    println!();
    println!("Next steps:");
    println!(
        "  1. Edit {}.json to configure your API endpoints",
        skill_id
    );
    println!("  2. Update skill.toml with correct permissions and secrets");
    println!("  3. Copy the .json file to your entity's skills/ directory");
    println!("     (or use --output to scaffold directly there)");

    Ok(())
}

fn scaffold_native_skill(dir: &std::path::Path, name: &str, skill_id: &str) -> anyhow::Result<()> {
    let crate_name = format!("skill-{}", name);

    // skill.toml manifest
    let toml_content = format!(
        r#"[skill]
id = "{skill_id}"
name = "{display_name}"
version = "0.1.0"
description = "TODO: describe what this skill does"
category = "General"

[runtime]
runtime = "Native"

[[permissions]]
permission = "Notifications"
reason = "Example permission"
optional = true
"#,
        skill_id = skill_id,
        display_name = to_title_case(name),
    );
    std::fs::write(dir.join("skill.toml"), toml_content)?;

    // Cargo.toml
    let cargo_content = format!(
        r#"[package]
name = "{crate_name}"
version.workspace = true
edition.workspace = true

[dependencies]
abigail-skills = {{ path = "../../crates/abigail-skills" }}
async-trait.workspace = true
serde_json.workspace = true
"#,
        crate_name = crate_name,
    );
    std::fs::write(dir.join("Cargo.toml"), cargo_content)?;

    // src/lib.rs
    std::fs::create_dir_all(dir.join("src"))?;
    let lib_content = format!(
        r#"//! {display_name} skill implementation.

use abigail_skills::prelude::*;
use std::collections::HashMap;

pub struct {struct_name} {{
    manifest: SkillManifest,
}}

impl {struct_name} {{
    pub fn new() -> Self {{
        Self {{
            manifest: SkillManifest {{
                id: SkillId("{skill_id}".to_string()),
                name: "{display_name}".to_string(),
                version: "0.1.0".to_string(),
                description: "TODO: describe what this skill does".to_string(),
                license: None,
                category: "General".to_string(),
                keywords: vec![],
                runtime: "Native".to_string(),
                min_abigail_version: "0.1.0".to_string(),
                platforms: vec!["All".to_string()],
                capabilities: vec![],
                permissions: vec![],
                secrets: vec![],
                config_defaults: HashMap::new(),
            }},
        }}
    }}
}}

#[async_trait::async_trait]
impl Skill for {struct_name} {{
    fn manifest(&self) -> &SkillManifest {{
        &self.manifest
    }}

    async fn initialize(&mut self, _config: SkillConfig) -> SkillResult<()> {{
        Ok(())
    }}

    async fn shutdown(&mut self) -> SkillResult<()> {{
        Ok(())
    }}

    fn health(&self) -> SkillHealth {{
        SkillHealth {{
            status: HealthStatus::Healthy,
            message: None,
            last_check: chrono::Utc::now(),
            metrics: HashMap::new(),
        }}
    }}

    fn tools(&self) -> Vec<ToolDescriptor> {{
        vec![ToolDescriptor {{
            name: "example_tool".to_string(),
            description: "TODO: describe this tool".to_string(),
            parameters: serde_json::json!({{
                "type": "object",
                "properties": {{
                    "input": {{ "type": "string", "description": "The input" }}
                }},
                "required": ["input"]
            }}),
            returns: serde_json::json!({{}}),
            cost_estimate: CostEstimate::default(),
            required_permissions: vec![],
            autonomous: true,
            requires_confirmation: false,
        }}]
    }}

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        _context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {{
        match tool_name {{
            "example_tool" => {{
                let input = params.values
                    .get("input")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(none)");
                Ok(ToolOutput::success(serde_json::json!({{
                    "echo": input
                }})))
            }}
            _ => Err(SkillError::ToolFailed(format!("Unknown tool: {{}}", tool_name))),
        }}
    }}

    fn capabilities(&self) -> Vec<CapabilityDescriptor> {{
        vec![]
    }}

    fn get_capability(&self, _cap_type: &str) -> Option<&dyn std::any::Any> {{
        None
    }}

    fn triggers(&self) -> Vec<TriggerDescriptor> {{
        vec![]
    }}
}}
"#,
        display_name = to_title_case(name),
        struct_name = to_pascal_case(name),
        skill_id = skill_id,
    );
    std::fs::write(dir.join("src").join("lib.rs"), lib_content)?;

    println!("  Created skill.toml (manifest)");
    println!("  Created Cargo.toml (crate config)");
    println!("  Created src/lib.rs (skill implementation)");
    println!();
    println!("Next steps:");
    println!(
        "  1. Add \"skills/{}\" to workspace members in root Cargo.toml",
        dir.file_name().unwrap().to_str().unwrap()
    );
    println!("  2. Implement your tool logic in src/lib.rs");
    println!("  3. Register the skill in entity-daemon's main.rs");

    Ok(())
}

/// Convert "my-skill-name" to "My Skill Name".
fn to_title_case(s: &str) -> String {
    s.split('-')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Convert "my-skill-name" to "MySkillName".
fn to_pascal_case(s: &str) -> String {
    s.split('-')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Returns a static timestamp string for scaffolded templates.
fn chrono_now_stub() -> String {
    "2026-01-01T00:00:00Z".to_string()
}
