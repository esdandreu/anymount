mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "anymount")]
#[command(about = "Mount cloud storage providers as local filesystems", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Mount mock filesystem (for testing and demonstration)
    Mock(commands::mock::MockCommand),

    /// Unmount a storage provider
    Unmount(commands::unmount::UnmountCommand),

    /// List active mounts
    List(commands::list::ListCommand),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("anymount={},anymount_core={},anymount_providers={}", log_level, log_level, log_level).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    match cli.command {
        Commands::Mock(cmd) => cmd.execute().await?,
        Commands::Unmount(cmd) => cmd.execute().await?,
        Commands::List(cmd) => cmd.execute().await?,
    }

    Ok(())
}

