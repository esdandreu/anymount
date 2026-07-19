use std::path::PathBuf;

use crate::ConfigDir;
use crate::cli::commands::connect::ConnectCommand;
use crate::tui;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "anymount")]
#[command(about = "Mount cloud storage providers as local filesystems", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Config directory override
    #[arg(short, long, global = true)]
    pub config_dir: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Connect a mount
    Connect(ConnectCommand),
}

impl Cli {
    pub fn run(self) -> color_eyre::Result<()> {
        match self.command {
            Some(Command::Connect(cmd)) => cmd.execute(),
            None => tui::run(
                self.config_dir
                    .map_or_else(ConfigDir::default, ConfigDir::new),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::commands::connect::ConnectSubcommand;
    use super::*;

    #[test]
    fn parse_connect_sync_name_command() {
        let cli = Cli::try_parse_from(["anymount", "connect", "demo"])
            .expect("parse should succeed");

        match cli.command.expect("command should exist") {
            Command::Connect(cmd) => match &cmd.action {
                Some(ConnectSubcommand::Named(tokens)) => {
                    assert_eq!(tokens, &vec!["demo".to_owned()]);
                }
                other => panic!("unexpected connect-sync action: {other:?}"),
            },
        }
    }
}
