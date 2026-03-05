use crate::cli::commands::auth::AuthCommand;
use crate::cli::commands::config::ConfigCommand;
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
    /// Obtain tokens for storage configuration (e.g. OneDrive).
    Auth(AuthCommand),
    /// Manage provider configuration files.
    Config(ConfigCommand),
    /// Connect to a storage provider
    Connect(ConnectCommand),
}

impl Cli {
    pub fn run(self) -> Result<(), String> {
        match self.command {
            Commands::Auth(cmd) => cmd.execute(),
            Commands::Config(cmd) => cmd.execute(),
            Commands::Connect(cmd) => cmd.execute(),
        }
    }
}
