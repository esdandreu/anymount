use crate::cli::commands::auth::AuthCommand;
use crate::cli::commands::config::ConfigCommand;
use crate::cli::commands::connect::ConnectCommand;
use crate::cli::commands::provide::ProvideCommand;
use crate::cli::commands::status::StatusCommand;
use crate::tui;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "anymount")]
#[command(about = "Mount cloud storage providers as local filesystems", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Obtain tokens for storage configuration (e.g. OneDrive).
    Auth(AuthCommand),
    /// Manage provider configuration files.
    Config(ConfigCommand),
    /// Connect to a storage provider
    Connect(ConnectCommand),
    /// Run one configured provider as a long-lived process.
    Provide(ProvideCommand),
    /// Show configured providers and daemon readiness.
    Status(StatusCommand),
}

impl Cli {
    pub fn run(self) -> super::Result<()> {
        match self.command {
            Some(Commands::Auth(cmd)) => cmd.execute(),
            Some(Commands::Config(cmd)) => cmd.execute(),
            Some(Commands::Connect(cmd)) => cmd.execute(),
            Some(Commands::Provide(cmd)) => cmd.execute(),
            Some(Commands::Status(cmd)) => cmd.execute(),
            None => tui::run().map_err(|error| super::Error::Validation(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_provide_name_command() {
        let cli = Cli::try_parse_from(["anymount", "provide", "--name", "demo"])
            .expect("parse should succeed");

        match cli.command.expect("command should exist") {
            Commands::Provide(cmd) => assert_eq!(cmd.name.as_deref(), Some("demo")),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_status_command() {
        let cli = Cli::try_parse_from(["anymount", "status"]).expect("parse should succeed");
        match cli.command.expect("command should exist") {
            Commands::Status(cmd) => assert!(cmd.config_dir.is_none()),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_provide_inline_command() {
        let cli = Cli::try_parse_from([
            "anymount",
            "provide",
            "--path",
            "/tmp/demo",
            "local",
            "/data/demo",
        ])
        .expect("parse should succeed");

        match cli.command.expect("command should exist") {
            Commands::Provide(cmd) => {
                assert!(cmd.name.is_none());
                assert_eq!(cmd.path.as_deref(), Some(std::path::Path::new("/tmp/demo")));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
