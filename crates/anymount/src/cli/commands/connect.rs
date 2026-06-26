use clap::{Args, Subcommand};
use color_eyre::eyre::eyre;
use std::path::PathBuf;

/// Connect command ensures configured driver processes are running.
#[derive(Args, Debug, Clone)]
pub struct ConnectCommand {
    /// Connect all configured drivers.
    #[arg(long)]
    pub all: bool,

    /// Config directory override.
    #[arg(long)]
    pub config_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub action: Option<ConnectSubcommand>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConnectSubcommand {
    /// Connect a configured driver by name (single token).
    #[command(external_subcommand)]
    Named(Vec<String>),
}

impl ConnectCommand {
    pub fn execute(&self) -> color_eyre::Result<()> {
        self.validate_arguments()?;
        if self.all {
            return Ok(());
        }

        match &self.action {
            None => Err(eyre!("missing connect target")),
            Some(ConnectSubcommand::Named(tokens)) => {
                let _name = single_external_subcommand_name(tokens)?;
                Ok(())
            }
        }
    }

    fn validate_arguments(&self) -> color_eyre::Result<()> {
        if self.all && self.action.is_some() {
            return Err(eyre!("--all cannot be combined with a subcommand"));
        }

        Ok(())
    }
}

fn single_external_subcommand_name(
    tokens: &[String],
) -> color_eyre::Result<&str> {
    match tokens {
        [name] => Ok(name),
        [] => Err(eyre!("missing connect target")),
        _ => Err(eyre!("connect target must be a single name")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_without_args_returns_error() {
        let cmd = ConnectCommand {
            all: false,
            config_dir: None,
            action: None,
        };

        assert!(cmd.execute().is_err());
    }

    #[test]
    fn all_with_subcommand_returns_error() {
        let cmd = ConnectCommand {
            all: true,
            config_dir: None,
            action: Some(ConnectSubcommand::Named(vec!["demo".to_owned()])),
        };

        assert!(cmd.execute().is_err());
    }

    #[test]
    fn named_connect_accepts_single_name() {
        let cmd = ConnectCommand {
            all: false,
            config_dir: None,
            action: Some(ConnectSubcommand::Named(vec!["demo".to_owned()])),
        };

        assert!(cmd.execute().is_ok());
    }

    #[test]
    fn named_connect_rejects_multiple_tokens() {
        let cmd = ConnectCommand {
            all: false,
            config_dir: None,
            action: Some(ConnectSubcommand::Named(vec![
                "a".to_owned(),
                "b".to_owned(),
            ])),
        };

        assert!(cmd.execute().is_err());
    }
}
