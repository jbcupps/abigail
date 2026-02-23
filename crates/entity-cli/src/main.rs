use clap::{Parser, Subcommand};
use entity_core::{
    ApiEnvelope, ChatRequest, ChatResponse, EntityStatus, RunRequest, RunResponse,
    DEFAULT_ENTITY_ADDR, ENTITY_API_VERSION_PREFIX,
};
use reqwest::Client;
use std::io::{self, Write};

#[derive(Parser)]
#[command(name = "entity-cli", about = "Entity runtime CLI")]
struct Cli {
    #[arg(long, default_value = DEFAULT_ENTITY_ADDR)]
    addr: String,
    #[arg(long, default_value_t = false)]
    oneshot: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Status,
    Run { task: String },
    Chat {
        message: Option<String>,
        #[arg(long, default_value_t = false)]
        interactive: bool,
    },
    Logs,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let command = cli.command;

    if cli.oneshot {
        return run_oneshot(command);
    }

    let client = Client::new();
    let base = format!("http://{}{}", cli.addr, ENTITY_API_VERSION_PREFIX);

    match command {
        Command::Status => {
            let res = client
                .get(format!("{base}/status"))
                .send()
                .await?
                .error_for_status()?
                .json::<ApiEnvelope<EntityStatus>>()
                .await?;
            println!(
                "service={} api={} mode={}",
                res.data.service, res.data.api_version, res.data.mode
            );
        }
        Command::Run { task } => {
            let res = client
                .post(format!("{base}/run"))
                .json(&RunRequest { task })
                .send()
                .await?
                .error_for_status()?
                .json::<ApiEnvelope<RunResponse>>()
                .await?;
            println!("accepted={} task={}", res.data.accepted, res.data.task);
        }
        Command::Chat {
            message,
            interactive,
        } => {
            if interactive {
                run_chat_repl(&client, &base).await?;
            } else {
                let message = message.ok_or_else(|| {
                    anyhow::anyhow!("chat message is required unless --interactive is used")
                })?;
                let res = client
                    .post(format!("{base}/chat"))
                    .json(&ChatRequest { message })
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<ApiEnvelope<ChatResponse>>()
                    .await?;
                println!("{}", res.data.reply);
            }
        }
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

fn run_oneshot(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Status => {
            println!("service=entity-cli api=v1 mode=oneshot");
        }
        Command::Run { task } => {
            println!("accepted=true task={task}");
        }
        Command::Chat {
            message,
            interactive,
        } => {
            if interactive {
                anyhow::bail!("oneshot mode does not support --interactive chat");
            }
            let message = message.ok_or_else(|| {
                anyhow::anyhow!("chat message is required unless --interactive is used")
            })?;
            println!("entity oneshot reply: {message}");
        }
        Command::Logs => {
            println!("oneshot mode has no daemon logs");
        }
    }
    Ok(())
}

async fn run_chat_repl(client: &Client, base: &str) -> anyhow::Result<()> {
    println!("interactive chat started (type 'exit' or '/exit' to quit)");
    let stdin = io::stdin();
    let mut line = String::new();

    loop {
        line.clear();
        print!("you> ");
        io::stdout().flush()?;

        let n = stdin.read_line(&mut line)?;
        if n == 0 {
            println!();
            break;
        }

        let message = line.trim();
        if message.is_empty() {
            continue;
        }
        if matches!(message, "exit" | "/exit" | "quit" | "/quit") {
            break;
        }

        let res = client
            .post(format!("{base}/chat"))
            .json(&ChatRequest {
                message: message.to_string(),
            })
            .send()
            .await?;
        let res = res
            .error_for_status()?
            .json::<ApiEnvelope<ChatResponse>>()
            .await?;
        println!("adam> {}", res.data.reply);
    }

    println!("interactive chat ended");
    Ok(())
}
