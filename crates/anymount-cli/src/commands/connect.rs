use anymount::connected_mount::{ConnectedMount, disconnect_all};
use anymount::{ConfigDir, ConfigRepository, LoadedConfig};
use clap::{Args, Subcommand};
use color_eyre::eyre::eyre;
use std::sync::Arc;

/// Connects configured mounts until the user presses Ctrl-C.
#[derive(Args, Debug, Clone)]
pub struct ConnectCommand {
    /// Connect all configured drivers.
    #[arg(long)]
    pub all: bool,

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
    pub async fn execute(
        &self,
        config_dir: Option<std::path::PathBuf>,
    ) -> color_eyre::Result<()> {
        let repository: Arc<dyn ConfigRepository> = Arc::new(
            config_dir.map_or_else(ConfigDir::default, ConfigDir::new),
        );
        let configurations = self.configurations(repository.as_ref())?;
        if configurations.is_empty() {
            return Err(eyre!("no configured mounts"));
        }
        let mut mounts = Vec::with_capacity(configurations.len());
        let result = async {
            for record in &configurations {
                mounts.push(
                    ConnectedMount::connect(record, Arc::clone(&repository))
                        .await?,
                );
                eprintln!(
                    "Connected '{}'. Press Ctrl-C to disconnect.",
                    record.config.mount.name
                );
            }
            tokio::signal::ctrl_c().await?;
            Ok(())
        }
        .await;
        result.and(disconnect_all(mounts).await.map_err(Into::into))
    }

    fn configurations(
        &self,
        repository: &dyn ConfigRepository,
    ) -> color_eyre::Result<Vec<LoadedConfig>> {
        match (self.all, &self.action) {
            (true, None) => Ok(repository.list().collect()),
            (true, Some(_)) => {
                Err(eyre!("--all cannot be combined with a subcommand"))
            }
            (false, None) => Err(eyre!("missing connect target")),
            (false, Some(ConnectSubcommand::Named(tokens))) => {
                let name = single_external_subcommand_name(tokens)?;
                Ok(vec![repository.find(name)?])
            }
        }
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
            action: None,
        };

        assert!(
            cmd.configurations(&ConfigDir::new("/missing".into()))
                .is_err()
        );
    }

    #[test]
    fn all_with_subcommand_returns_error() {
        let cmd = ConnectCommand {
            all: true,
            action: Some(ConnectSubcommand::Named(vec!["demo".to_owned()])),
        };

        assert!(
            cmd.configurations(&ConfigDir::new("/missing".into()))
                .is_err()
        );
    }

    #[test]
    fn named_connect_accepts_single_name() {
        let cmd = ConnectCommand {
            all: false,
            action: Some(ConnectSubcommand::Named(vec!["demo".to_owned()])),
        };

        assert_eq!(
            single_external_subcommand_name(match &cmd.action {
                Some(ConnectSubcommand::Named(tokens)) => tokens,
                None => panic!("missing target"),
            })
            .expect("single target"),
            "demo",
        );
    }

    #[test]
    fn named_connect_rejects_multiple_tokens() {
        let cmd = ConnectCommand {
            all: false,
            action: Some(ConnectSubcommand::Named(vec![
                "a".to_owned(),
                "b".to_owned(),
            ])),
        };

        assert!(
            cmd.configurations(&ConfigDir::new("/missing".into()))
                .is_err()
        );
    }
}
