use std::path::PathBuf;

use crate::commands::auth::AuthCommand;
use crate::commands::connect::ConnectCommand;
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
    /// Sign in and save credentials for a named configuration.
    Auth(AuthCommand),
}

impl Cli {
    pub async fn run(self) -> color_eyre::Result<()> {
        match self.command {
            Some(Command::Connect(cmd)) => cmd.execute(self.config_dir).await,
            Some(Command::Auth(cmd)) => cmd.execute(self.config_dir).await,
            None => anymount_tui::Tui::new(self.config_dir).run().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::connect::ConnectSubcommand;

    #[test]
    fn parses_config_directory() {
        let cli = Cli::try_parse_from([
            "anymount",
            "--config-dir",
            "/tmp/anymount-config",
        ])
        .expect("parse should succeed");

        assert_eq!(cli.config_dir, Some(PathBuf::from("/tmp/anymount-config")));
    }

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
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_named_auth_with_global_config_directory() {
        for arguments in [
            vec!["anymount", "--config-dir", "/tmp/custom", "auth", "drive"],
            vec!["anymount", "auth", "drive", "--config-dir", "/tmp/custom"],
        ] {
            let cli = Cli::try_parse_from(arguments).expect("parse auth");
            assert_eq!(cli.config_dir, Some(PathBuf::from("/tmp/custom")));
            match cli.command.expect("command") {
                Command::Auth(command) => assert_eq!(command.name, "drive"),
                other => panic!("unexpected command: {other:?}"),
            }
        }
    }

    #[test]
    fn rejects_auth_without_a_configuration_name() {
        assert!(Cli::try_parse_from(["anymount", "auth"]).is_err());
    }
}
