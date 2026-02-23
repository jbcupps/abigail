//! Hive CLI — thin reqwest client for hive-daemon.
//!
//! Usage:
//!   hive-cli status
//!   hive-cli entities
//!   hive-cli create <name>
//!   hive-cli entity <id>
//!   hive-cli provider-config <id>
//!   hive-cli store-secret <key> <value>
//!   hive-cli secrets

use clap::{Parser, Subcommand};
use hive_core::{
    ApiEnvelope, CreateEntityRequest, CreateEntityResponse, EntityInfo, HiveStatus, ProviderConfig,
    SecretListResponse, StoreSecretRequest,
};

#[derive(Parser)]
#[command(name = "hive-cli", about = "CLI client for Abigail Hive daemon")]
struct Cli {
    /// Hive daemon URL
    #[arg(long, default_value = "http://127.0.0.1:3141", global = true)]
    url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show Hive status
    Status,
    /// List all entities
    Entities,
    /// Create a new entity
    Create {
        /// Name for the new entity
        name: String,
    },
    /// Get entity details
    Entity {
        /// Entity UUID
        id: String,
    },
    /// Get provider config for an entity
    ProviderConfig {
        /// Entity UUID
        id: String,
    },
    /// Store a secret in the Hive vault
    StoreSecret {
        /// Secret key
        key: String,
        /// Secret value
        value: String,
    },
    /// List secret names
    Secrets,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();
    let base = cli.url.trim_end_matches('/');

    match cli.command {
        Commands::Status => {
            let resp: ApiEnvelope<HiveStatus> = client
                .get(format!("{}/v1/status", base))
                .send()
                .await?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Commands::Entities => {
            let resp: ApiEnvelope<Vec<EntityInfo>> = client
                .get(format!("{}/v1/entities", base))
                .send()
                .await?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Commands::Create { name } => {
            let resp: ApiEnvelope<CreateEntityResponse> = client
                .post(format!("{}/v1/entities", base))
                .json(&CreateEntityRequest { name })
                .send()
                .await?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Commands::Entity { id } => {
            let resp: ApiEnvelope<EntityInfo> = client
                .get(format!("{}/v1/entities/{}", base, id))
                .send()
                .await?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Commands::ProviderConfig { id } => {
            let resp: ApiEnvelope<ProviderConfig> = client
                .get(format!("{}/v1/entities/{}/provider-config", base, id))
                .send()
                .await?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Commands::StoreSecret { key, value } => {
            let resp: ApiEnvelope<String> = client
                .post(format!("{}/v1/secrets", base))
                .json(&StoreSecretRequest { key, value })
                .send()
                .await?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Commands::Secrets => {
            let resp: ApiEnvelope<SecretListResponse> = client
                .get(format!("{}/v1/secrets/list", base))
                .send()
                .await?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
    }

    Ok(())
}
