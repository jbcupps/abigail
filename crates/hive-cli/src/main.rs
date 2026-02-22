use anyhow::Context;
use clap::{Parser, Subcommand};
use hive_core::{
    ApiEnvelope, EntityRecord, HiveStatus, StartStopEntityRequest, DEFAULT_HIVE_ADDR,
    HIVE_API_VERSION_PREFIX,
};
use reqwest::Client;

#[derive(Parser)]
#[command(name = "hive-cli", about = "Hive control plane CLI")]
struct Cli {
    #[arg(long, default_value = DEFAULT_HIVE_ADDR)]
    addr: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Status,
    Entity {
        #[command(subcommand)]
        command: EntityCommand,
    },
    Logs,
}

#[derive(Subcommand)]
enum EntityCommand {
    List,
    Start { id: String },
    Stop { id: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let client = Client::new();
    let base = format!("http://{}{}", cli.addr, HIVE_API_VERSION_PREFIX);

    match cli.command {
        Command::Status => {
            let res = client
                .get(format!("{base}/status"))
                .send()
                .await?
                .error_for_status()?
                .json::<ApiEnvelope<HiveStatus>>()
                .await?;
            println!(
                "service={} api={} managed_entities={}",
                res.data.service, res.data.api_version, res.data.managed_entities
            );
        }
        Command::Entity { command } => match command {
            EntityCommand::List => {
                let res = client
                    .get(format!("{base}/entity/list"))
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<ApiEnvelope<Vec<EntityRecord>>>()
                    .await?;
                if res.data.is_empty() {
                    println!("no entities registered");
                } else {
                    for entity in res.data {
                        println!("{} {:?}", entity.id, entity.status);
                    }
                }
            }
            EntityCommand::Start { id } => {
                let payload = StartStopEntityRequest { id };
                let res = client
                    .post(format!("{base}/entity/start"))
                    .json(&payload)
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<ApiEnvelope<EntityRecord>>()
                    .await
                    .context("failed to parse hive start response")?;
                println!("started entity {}", res.data.id);
            }
            EntityCommand::Stop { id } => {
                let payload = StartStopEntityRequest { id };
                let res = client
                    .post(format!("{base}/entity/stop"))
                    .json(&payload)
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<ApiEnvelope<EntityRecord>>()
                    .await
                    .context("failed to parse hive stop response")?;
                println!("stopped entity {}", res.data.id);
            }
        },
        Command::Logs => {
            let res = client
                .get(format!("{base}/logs"))
                .send()
                .await?
                .error_for_status()?
                .json::<ApiEnvelope<Vec<String>>>()
                .await?;
            for line in res.data {
                println!("{line}");
            }
        }
    }

    Ok(())
}
