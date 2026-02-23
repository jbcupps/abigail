//! Entity CLI — thin reqwest client for entity-daemon.
//!
//! Usage:
//!   entity-cli status
//!   entity-cli chat "hello"
//!   entity-cli skills
//!   entity-cli tool <skill_id> <tool_name> [params_json]

use clap::{Parser, Subcommand};
use entity_core::{
    ApiEnvelope, ChatRequest, ChatResponse, EntityStatus, SkillInfo, ToolExecRequest,
    ToolExecResponse,
};

#[derive(Parser)]
#[command(name = "entity-cli", about = "CLI client for Abigail Entity daemon")]
struct Cli {
    /// Entity daemon URL
    #[arg(long, default_value = "http://127.0.0.1:3142", global = true)]
    url: String,

    #[command(subcommand)]
    command: Commands,
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
    }

    Ok(())
}
