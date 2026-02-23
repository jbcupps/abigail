//! Abigail CLI: troubleshooting interface for direct system interaction.
//!
//! Provides CLI subcommands and a REST API server for credential storage,
//! status checks, and diagnostics — without requiring the Tauri runtime.

mod auth;
mod commands;
mod server;
mod shell;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "abigail",
    about = "Abigail CLI shell and troubleshooting tools"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the interactive onboarding + chat shell
    Shell,
    /// Show agent status (config, router, skills, integrations)
    Status,
    /// Store a secret in the vault
    StoreSecret {
        /// Secret key name
        key: String,
        /// Secret value
        value: String,
    },
    /// Check if a secret exists in the vault
    CheckSecret {
        /// Secret key name
        key: String,
    },
    /// List all registered secret key names
    ListSecrets,
    /// Configure IMAP/SMTP credentials
    ConfigureEmail {
        /// Email address
        #[arg(long)]
        address: String,
        /// IMAP server hostname
        #[arg(long)]
        imap_host: String,
        /// IMAP server port
        #[arg(long)]
        imap_port: u16,
        /// SMTP server hostname
        #[arg(long)]
        smtp_host: String,
        /// SMTP server port
        #[arg(long)]
        smtp_port: u16,
        /// Email password
        #[arg(long)]
        password: String,
    },
    /// Show preloaded integration status
    IntegrationStatus,
    /// Show Id/Ego/Superego routing status
    RouterStatus,
    /// Start the REST API server
    Serve {
        /// Port to listen on
        #[arg(long, default_value = "3141")]
        port: u16,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    let result = match cli.command {
        None | Some(Commands::Shell) => shell::run_shell().await,
        Some(Commands::Status) => commands::status(),
        Some(Commands::StoreSecret { key, value }) => commands::store_secret(&key, &value),
        Some(Commands::CheckSecret { key }) => commands::check_secret(&key),
        Some(Commands::ListSecrets) => commands::list_secrets(),
        Some(Commands::ConfigureEmail {
            address,
            imap_host,
            imap_port,
            smtp_host,
            smtp_port,
            password,
        }) => commands::configure_email(
            &address, &imap_host, imap_port, &smtp_host, smtp_port, &password,
        ),
        Some(Commands::IntegrationStatus) => commands::integration_status(),
        Some(Commands::RouterStatus) => commands::router_status(),
        Some(Commands::Serve { port }) => server::serve(port).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        return Err(e);
    }

    Ok(())
}
