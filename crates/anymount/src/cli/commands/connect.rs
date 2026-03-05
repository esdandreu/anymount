use crate::config::ConfigDir;
use crate::{
    Config, Logger, Provider, ProviderFileConfig, ProvidersConfiguration, StorageConfig,
    TracingLogger,
};
use clap::{Args, Subcommand};
use std::path::PathBuf;
use std::sync::mpsc;

/// Connect command. Providers can come from config files
/// (`--name` / `--all`) or inline CLI args (`--path` + storage
/// subcommand).
#[derive(Args, Debug, Clone)]
pub struct ConnectCommand {
    /// Connect a named provider from config.
    #[arg(long, conflicts_with_all = ["all", "path"])]
    pub name: Option<String>,

    /// Connect all configured providers.
    #[arg(long, conflicts_with_all = ["name", "path"])]
    pub all: bool,

    /// Path to the mount point (inline mode).
    #[arg(long, conflicts_with_all = ["name", "all"])]
    pub path: Option<PathBuf>,

    /// Config directory override.
    #[arg(long)]
    pub config_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub storage: Option<ConnectStorageSubcommand>,
}

impl ConnectCommand {
    pub fn execute(&self) -> Result<(), String> {
        let logger = TracingLogger::new();
        self._execute(&DefaultProviderConnector, &DefaultStopSignalWaiter, &logger)
    }

    pub(crate) fn _execute<C, W, L>(
        &self,
        connector: &C,
        waiter: &W,
        logger: &L,
    ) -> Result<(), String>
    where
        C: ProviderConnector,
        W: StopSignalWaiter,
        L: Logger + 'static,
    {
        if self.all {
            let cd = self.config_dir();
            let config = cd.load_all()?;
            run_providers(&config, connector, waiter, logger)
        } else if let Some(name) = &self.name {
            let cd = self.config_dir();
            let provider = cd.read(name)?;
            let config = Config {
                providers: vec![provider],
            };
            run_providers(&config, connector, waiter, logger)
        } else if let (Some(path), Some(storage)) = (&self.path, &self.storage) {
            let inline = InlineConfig {
                path: path.clone(),
                storage: storage.to_storage_config(),
            };
            let config = Config {
                providers: vec![ProviderFileConfig {
                    path: inline.path,
                    storage: inline.storage,
                }],
            };
            run_providers(&config, connector, waiter, logger)
        } else {
            Err("specify --name <NAME>, --all, or \
                 --path <PATH> with a storage subcommand"
                .to_owned())
        }
    }

    fn config_dir(&self) -> ConfigDir {
        match &self.config_dir {
            Some(p) => ConfigDir::new(p.clone()),
            None => ConfigDir::default(),
        }
    }
}

struct InlineConfig {
    path: PathBuf,
    storage: StorageConfig,
}

/// Shared connection logic: connect providers, wait for stop signal,
/// then clean up.
pub(crate) fn run_providers<P, C, W, L>(
    config: &P,
    connector: &C,
    waiter: &W,
    logger: &L,
) -> Result<(), String>
where
    P: ProvidersConfiguration,
    C: ProviderConnector,
    W: StopSignalWaiter,
    L: Logger + 'static,
{
    let providers = connector.connect(config, logger)?;
    for provider in &providers {
        logger.info(format!(
            "Connected to {} at {}",
            provider.kind(),
            provider.path().display()
        ));
    }
    logger.info("All providers connected. Press Ctrl+C to disconnect.");
    waiter.wait(logger);
    drop(providers);
    Ok(())
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConnectStorageSubcommand {
    /// Local directory as storage backend
    Local(LocalStorageArgs),
    /// OneDrive (Microsoft Graph) as storage backend
    #[command(name = "onedrive")]
    OneDrive(OneDriveStorageArgs),
}

#[derive(Args, Debug, Clone)]
pub struct LocalStorageArgs {
    /// Root directory to expose
    #[arg(value_name = "ROOT")]
    pub root: PathBuf,
}

#[derive(Args, Debug, Clone)]
pub struct OneDriveStorageArgs {
    /// OneDrive path to use as root (e.g. / or /Documents)
    #[arg(long, default_value = "/", value_name = "PATH")]
    pub root: PathBuf,
    /// Graph API endpoint (e.g. https://graph.microsoft.com/v1.0)
    #[arg(
        long,
        default_value = "https://graph.microsoft.com/v1.0",
        value_name = "URL"
    )]
    pub endpoint: String,
    /// Access token (optional if refresh_token and client_id are set)
    #[arg(long, value_name = "TOKEN")]
    pub access_token: Option<String>,
    /// Refresh token (required if access_token is missing or may expire)
    #[arg(long, value_name = "TOKEN")]
    pub refresh_token: Option<String>,
    /// OAuth client_id (required when refresh_token is set)
    #[arg(long, value_name = "ID")]
    pub client_id: Option<String>,
    /// Seconds before token expiry to trigger refresh (default: 60)
    #[arg(long, default_value = "60", value_name = "SECS")]
    pub token_expiry_buffer_secs: u64,
}

impl ConnectStorageSubcommand {
    pub(crate) fn to_storage_config(&self) -> StorageConfig {
        match self {
            Self::Local(args) => StorageConfig::Local {
                root: args.root.clone(),
            },
            Self::OneDrive(args) => StorageConfig::OneDrive {
                root: args.root.clone(),
                endpoint: args.endpoint.clone(),
                access_token: args.access_token.clone(),
                refresh_token: args.refresh_token.clone(),
                client_id: args.client_id.clone(),
                token_expiry_buffer_secs: Some(args.token_expiry_buffer_secs),
            },
        }
    }
}

/// Port for connecting to storage providers. Inject a mock in tests.
pub trait ProviderConnector {
    fn connect<C, L>(&self, config: &C, logger: &L) -> Result<Vec<Box<dyn Provider>>, String>
    where
        C: ProvidersConfiguration,
        L: Logger + 'static;
}

/// Default connector that uses the platform cloud filter (e.g. Windows Cloud
/// Filter API).
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultProviderConnector;

impl ProviderConnector for DefaultProviderConnector {
    fn connect<C, L>(&self, config: &C, logger: &L) -> Result<Vec<Box<dyn Provider>>, String>
    where
        C: ProvidersConfiguration,
        L: Logger + 'static,
    {
        crate::connect_providers(config, logger)
    }
}

/// Port for blocking until the user requests disconnect (e.g. Ctrl+C). Inject
/// a no-op in tests.
pub trait StopSignalWaiter {
    fn wait<L: Logger>(&self, logger: &L);
}

/// Default waiter that blocks until Ctrl+C.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultStopSignalWaiter;

impl StopSignalWaiter for DefaultStopSignalWaiter {
    fn wait<L: Logger>(&self, logger: &L) {
        let (tx, rx) = mpsc::channel();
        if let Err(e) = ctrlc::set_handler(move || {
            let _ = tx.send(());
        }) {
            logger.error(format!("Error setting Ctrl-C handler: {}", e));
            return;
        }
        let _ = rx.recv();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoOpLogger;

    struct FailingConnector;

    impl ProviderConnector for FailingConnector {
        fn connect<C, L>(
            &self,
            _config: &C,
            _logger: &L,
        ) -> Result<Vec<Box<dyn crate::Provider>>, String>
        where
            C: crate::ProvidersConfiguration,
            L: crate::Logger + 'static,
        {
            Err("mock connect error".into())
        }
    }

    struct NoOpWaiter;

    impl StopSignalWaiter for NoOpWaiter {
        fn wait<L: crate::Logger>(&self, _logger: &L) {}
    }

    fn inline_cmd() -> ConnectCommand {
        ConnectCommand {
            name: None,
            all: false,
            path: Some(PathBuf::from("/tmp/mount")),
            config_dir: None,
            storage: Some(ConnectStorageSubcommand::Local(LocalStorageArgs {
                root: PathBuf::from("/tmp/root"),
            })),
        }
    }

    #[test]
    fn execute_returns_connector_error() {
        let cmd = inline_cmd();
        let logger = NoOpLogger;
        let err = cmd
            ._execute(&FailingConnector, &NoOpWaiter, &logger)
            .unwrap_err();
        assert_eq!(err, "mock connect error");
    }

    struct EmptyConnector;

    impl ProviderConnector for EmptyConnector {
        fn connect<C, L>(
            &self,
            _config: &C,
            _logger: &L,
        ) -> Result<Vec<Box<dyn crate::Provider>>, String>
        where
            C: crate::ProvidersConfiguration,
            L: crate::Logger + 'static,
        {
            Ok(vec![])
        }
    }

    #[test]
    fn execute_succeeds_with_inline_args() {
        let cmd = inline_cmd();
        let logger = NoOpLogger;
        let result = cmd._execute(&EmptyConnector, &NoOpWaiter, &logger);
        assert!(result.is_ok());
    }

    #[test]
    fn execute_from_config_name() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        cd.write(
            "test",
            &ProviderFileConfig {
                path: PathBuf::from("/mnt/test"),
                storage: StorageConfig::Local {
                    root: PathBuf::from("/data"),
                },
            },
        )
        .expect("write failed");

        let cmd = ConnectCommand {
            name: Some("test".to_owned()),
            all: false,
            path: None,
            config_dir: Some(tmp.path().to_path_buf()),
            storage: None,
        };
        let logger = NoOpLogger;
        let result = cmd._execute(&EmptyConnector, &NoOpWaiter, &logger);
        assert!(result.is_ok());
    }

    #[test]
    fn execute_all_from_config() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        cd.write(
            "a",
            &ProviderFileConfig {
                path: PathBuf::from("/mnt/a"),
                storage: StorageConfig::Local {
                    root: PathBuf::from("/data/a"),
                },
            },
        )
        .expect("write failed");

        let cmd = ConnectCommand {
            name: None,
            all: true,
            path: None,
            config_dir: Some(tmp.path().to_path_buf()),
            storage: None,
        };
        let logger = NoOpLogger;
        let result = cmd._execute(&EmptyConnector, &NoOpWaiter, &logger);
        assert!(result.is_ok());
    }

    #[test]
    fn execute_without_args_returns_error() {
        let cmd = ConnectCommand {
            name: None,
            all: false,
            path: None,
            config_dir: None,
            storage: None,
        };
        let logger = NoOpLogger;
        let result = cmd._execute(&EmptyConnector, &NoOpWaiter, &logger);
        assert!(result.is_err());
    }
}
