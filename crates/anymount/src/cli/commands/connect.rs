use crate::{Provider, ProviderConfiguration, ProvidersConfiguration, StorageConfig};
use clap::{Args, Subcommand};
use std::path::PathBuf;
use std::sync::mpsc;
use tracing::{error, info};

#[derive(Args, Debug, Clone)]
pub struct ConnectCommand {
    /// Path to the mount point
    #[arg(long)]
    pub path: PathBuf,
    #[command(subcommand)]
    pub storage: ConnectStorageSubcommand,
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
    fn to_storage_config(&self) -> StorageConfig {
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

impl ProviderConfiguration for ConnectCommand {
    fn path(&self) -> PathBuf {
        self.path.clone()
    }

    fn storage_config(&self) -> StorageConfig {
        self.storage.to_storage_config()
    }
}

impl ProvidersConfiguration for ConnectCommand {
    fn providers(&self) -> Vec<&impl ProviderConfiguration> {
        vec![self]
    }
}

impl ConnectCommand {
    /// Runs with default connector and waiter.
    pub fn execute(&self) -> Result<(), String> {
        self._execute(&DefaultProviderConnector, &DefaultStopSignalWaiter)
    }

    /// Internal entry point for injection (e.g. tests). Not part of the public API.
    pub(crate) fn _execute<C, W>(&self, connector: &C, waiter: &W) -> Result<(), String>
    where
        C: ProviderConnector,
        W: StopSignalWaiter,
    {
        let providers = connector.connect(self)?;
        for provider in &providers {
            info!(
                "Connected to {} at {}",
                provider.kind(),
                provider.path().display()
            );
        }
        info!("All providers connected. Press Ctrl+C to disconnect.");
        waiter.wait();
        // Keep providers alive until here
        drop(providers);
        Ok(())
    }
}

/// Port for connecting to storage providers. Inject a mock in tests.
pub trait ProviderConnector {
    fn connect<C>(&self, config: &C) -> Result<Vec<Box<dyn Provider>>, String>
    where
        C: ProvidersConfiguration;
}

/// Default connector that uses the platform cloud filter (e.g. Windows Cloud
/// Filter API).
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultProviderConnector;

impl ProviderConnector for DefaultProviderConnector {
    fn connect<C>(&self, config: &C) -> Result<Vec<Box<dyn Provider>>, String>
    where
        C: ProvidersConfiguration,
    {
        crate::connect_providers(config)
    }
}

/// Port for blocking until the user requests disconnect (e.g. Ctrl+C). Inject
/// a no-op in tests.
pub trait StopSignalWaiter {
    fn wait(&self);
}

/// Default waiter that blocks until Ctrl+C.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultStopSignalWaiter;

impl StopSignalWaiter for DefaultStopSignalWaiter {
    fn wait(&self) {
        let (tx, rx) = mpsc::channel();
        if let Err(e) = ctrlc::set_handler(move || {
            let _ = tx.send(());
        }) {
            error!("Error setting Ctrl-C handler: {}", e);
            return;
        }
        let _ = rx.recv();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingConnector;

    impl ProviderConnector for FailingConnector {
        fn connect<C>(&self, _config: &C) -> Result<Vec<Box<dyn crate::Provider>>, String>
        where
            C: crate::ProvidersConfiguration,
        {
            Err("mock connect error".into())
        }
    }

    struct NoOpWaiter;

    impl StopSignalWaiter for NoOpWaiter {
        fn wait(&self) {}
    }

    #[test]
    fn execute_returns_connector_error_without_calling_real_connector() {
        let cmd = ConnectCommand {
            path: PathBuf::from("/tmp/mount"),
            storage: ConnectStorageSubcommand::Local(LocalStorageArgs {
                root: PathBuf::from("/tmp/root"),
            }),
        };
        let err = cmd._execute(&FailingConnector, &NoOpWaiter).unwrap_err();
        assert_eq!(err, "mock connect error");
    }

    struct EmptyConnector;

    impl ProviderConnector for EmptyConnector {
        fn connect<C>(&self, _config: &C) -> Result<Vec<Box<dyn crate::Provider>>, String>
        where
            C: crate::ProvidersConfiguration,
        {
            Ok(vec![])
        }
    }

    #[test]
    fn execute_succeeds_with_empty_connector_and_noop_waiter() {
        let cmd = ConnectCommand {
            path: PathBuf::from("/tmp/mount"),
            storage: ConnectStorageSubcommand::Local(LocalStorageArgs {
                root: PathBuf::from("/tmp/root"),
            }),
        };
        let result = cmd._execute(&EmptyConnector, &NoOpWaiter);
        assert!(result.is_ok());
    }
}
