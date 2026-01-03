use crate::cli::commands::connect::ConnectCommand;
use clap::{Parser, Subcommand};
use std::result::Result;

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
    /// Connect to a storage provider
    Connect(ConnectCommand),
}

impl Cli {
    pub async fn run(self) -> Result<(), String> {
        match self.command {
            Commands::Connect(cmd) => cmd.execute().await,
        }
    }
}
