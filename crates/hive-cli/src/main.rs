use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use hive_core::{
    ApiEnvelope, BirthEntityRequest, BirthPath, EntityRecord, HiveStatus, StartStopEntityRequest,
    DEFAULT_HIVE_ADDR, HIVE_API_VERSION_PREFIX,
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
    Birth {
        id: String,
        #[arg(long, value_enum, default_value_t = BirthPathArg::QuickStart)]
        path: BirthPathArg,
    },
    Select {
        id: String,
    },
    Start {
        id: String,
    },
    Stop {
        id: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BirthPathArg {
    QuickStart,
    Direct,
    SoulCrystallization,
    SoulForge,
}

impl From<BirthPathArg> for BirthPath {
    fn from(value: BirthPathArg) -> Self {
        match value {
            BirthPathArg::QuickStart => BirthPath::QuickStart,
            BirthPathArg::Direct => BirthPath::Direct,
            BirthPathArg::SoulCrystallization => BirthPath::SoulCrystallization,
            BirthPathArg::SoulForge => BirthPath::SoulForge,
        }
    }
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
                        let birth_path = entity
                            .birth_path
                            .map(|p| format!("{:?}", p))
                            .unwrap_or_else(|| "none".to_string());
                        println!(
                            "{} status={:?} birth_complete={} birth_path={}",
                            entity.id, entity.status, entity.birth_complete, birth_path
                        );
                    }
                }
            }
            EntityCommand::Birth { id, path } => {
                let payload = BirthEntityRequest {
                    id,
                    path: path.into(),
                };
                let res = client
                    .post(format!("{base}/entity/birth"))
                    .json(&payload)
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<ApiEnvelope<EntityRecord>>()
                    .await
                    .context("failed to parse hive birth response")?;
                println!(
                    "birthed entity {} via {:?}",
                    res.data.id,
                    res.data.birth_path.unwrap_or(BirthPath::QuickStart)
                );
            }
            EntityCommand::Select { id } => {
                let payload = StartStopEntityRequest { id };
                let res = client
                    .post(format!("{base}/entity/select"))
                    .json(&payload)
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<ApiEnvelope<EntityRecord>>()
                    .await
                    .context("failed to parse hive select response")?;
                println!(
                    "selected entity {} status={:?}",
                    res.data.id, res.data.status
                );
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
