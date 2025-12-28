pub mod commands;

use anymount::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "anymount")]
#[command(about = "Mount cloud storage providers as local filesystems", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Unmount a storage provider
    Unmount(commands::unmount::UnmountCommand),

    /// List active mounts
    List(commands::list::ListCommand),
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Commands::Unmount(cmd) => cmd.execute().await,
            Commands::List(cmd) => cmd.execute().await,
        }
    }
}
