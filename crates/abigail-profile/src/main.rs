//! Offline preview-only cold recovery; never launches a daemon or imports an Entity.
use abigail_core::profile::production_roots;
use abigail_core::profile_snapshot;
use abigail_core::vault::unlock::SystemRestoreCredentialProbe;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "abigail-profile",
    about = "Cold preview-profile recovery on this Windows device"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Copy a stopped preview to a new snapshot directory. No credential access.
    Snapshot {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Check manifest, every file/hash and identity/config consistency. No credential access.
    Verify {
        #[arg(long)]
        snapshot: PathBuf,
    },
    /// Recover into a new root using existing same-user/device credentials.
    /// Does not bootstrap credentials, overwrite a root, or launch an application.
    Restore {
        #[arg(long)]
        snapshot: PathBuf,
        #[arg(long)]
        destination: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Snapshot { source, output } => {
            let manifest = profile_snapshot::snapshot(&source, &output, &production_roots())?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
        }
        Command::Verify { snapshot } => {
            let manifest = profile_snapshot::verify_snapshot(&snapshot)?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
        }
        Command::Restore {
            snapshot,
            destination,
        } => {
            let receipt = profile_snapshot::restore(
                &snapshot,
                &destination,
                &production_roots(),
                &SystemRestoreCredentialProbe,
            )?;
            println!("{}", serde_json::to_string_pretty(&receipt)?);
        }
    }
    Ok(())
}
