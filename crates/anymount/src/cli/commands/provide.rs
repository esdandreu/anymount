use crate::config::ConfigDir;
use crate::daemon::messages::DaemonMessage;
use crate::daemon::runtime::DaemonRuntime;
use crate::{Config, Logger, Provider, ProviderFileConfig, StorageConfig, TracingLogger};
use clap::{Args, Subcommand};
use std::path::PathBuf;
use std::sync::mpsc;

#[cfg(unix)]
use crate::daemon::control_unix::UnixControl;
use crate::daemon::messages::ControlMessage;

#[cfg(target_os = "windows")]
use crate::daemon::control_windows::WindowsControl;

#[derive(Args, Debug, Clone)]
pub struct ProvideCommand {
    #[arg(long, conflicts_with = "path")]
    pub name: Option<String>,

    #[arg(long, conflicts_with = "name")]
    pub path: Option<PathBuf>,

    #[arg(long)]
    pub config_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub storage: Option<ProvideStorageSubcommand>,
}

impl ProvideCommand {
    pub fn execute(&self) -> crate::cli::Result<()> {
        self.run_with(&DefaultProvideRunner)
    }

    pub(crate) fn run_with<R>(&self, runner: &R) -> crate::cli::Result<()>
    where
        R: ProvideRunner,
    {
        runner.run(self, &TracingLogger::new())
    }
}

pub trait ProvideRunner {
    fn run<L: Logger + 'static>(
        &self,
        command: &ProvideCommand,
        logger: &L,
    ) -> crate::cli::Result<()>;
}

#[derive(Subcommand, Debug, Clone)]
pub enum ProvideStorageSubcommand {
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

impl ProvideStorageSubcommand {
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

#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultProvideRunner;

impl ProvideRunner for DefaultProvideRunner {
    fn run<L: Logger + 'static>(
        &self,
        command: &ProvideCommand,
        logger: &L,
    ) -> crate::cli::Result<()> {
        let request = command.resolve_request()?;
        let (tx, rx) = mpsc::channel();
        install_ctrlc_handler(tx.clone(), logger)?;

        let providers: Vec<Box<dyn Provider>> =
            crate::connect_providers_with_telemetry(&request.config, logger, Some(tx.clone()))?;

        for provider in &providers {
            logger.info(format!(
                "Connected to {} at {}",
                provider.kind(),
                provider.path().display()
            ));
        }

        if let Some(provider_name) = request.provider_name.as_deref() {
            spawn_control_server(provider_name, tx, logger)?;
        }

        let mut runtime = DaemonRuntime::new(logger.clone(), rx);
        let result = runtime.run().map_err(crate::cli::Error::from);
        drop(providers);
        result
    }
}

#[derive(Debug, Clone)]
struct ProvideRequest {
    provider_name: Option<String>,
    config: Config,
}

impl ProvideCommand {
    fn resolve_request(&self) -> crate::cli::Result<ProvideRequest> {
        if let Some(name) = &self.name {
            let cd = self.config_dir();
            let provider = cd.read(name)?;
            return Ok(ProvideRequest {
                provider_name: Some(name.clone()),
                config: Config {
                    providers: vec![provider],
                },
            });
        }

        let Some(path) = &self.path else {
            return Err(crate::cli::Error::MissingProvideTarget);
        };
        let Some(storage) = &self.storage else {
            return Err(crate::cli::Error::MissingProvideTarget);
        };

        Ok(ProvideRequest {
            provider_name: None,
            config: Config {
                providers: vec![ProviderFileConfig {
                    path: path.clone(),
                    storage: storage.to_storage_config(),
                }],
            },
        })
    }

    fn config_dir(&self) -> ConfigDir {
        match &self.config_dir {
            Some(path) => ConfigDir::new(path.clone()),
            None => ConfigDir::default(),
        }
    }
}

fn control_reply_for_request(
    bytes: &[u8],
    provider_name: &str,
    tx: &mpsc::Sender<DaemonMessage>,
) -> ControlMessage {
    match ControlMessage::decode(bytes) {
        Ok(ControlMessage::Ping) => ControlMessage::Ready,
        Ok(ControlMessage::Shutdown) => {
            let _ = tx.send(DaemonMessage::Shutdown);
            ControlMessage::Ack
        }
        Ok(other) => ControlMessage::Error(format!(
            "unsupported control message for {provider_name}: {other:?}"
        )),
        Err(error) => ControlMessage::Error(error.to_string()),
    }
}

fn install_ctrlc_handler<L: Logger>(
    tx: mpsc::Sender<DaemonMessage>,
    logger: &L,
) -> crate::cli::Result<()> {
    ctrlc::set_handler(move || {
        let _ = tx.send(DaemonMessage::Shutdown);
    })
    .map_err(|source| {
        logger.error(format!("Error setting Ctrl-C handler: {source}"));
        crate::cli::Error::InstallCtrlC { source }
    })
}

#[cfg(unix)]
fn spawn_control_server<L: Logger + 'static>(
    provider_name: &str,
    tx: mpsc::Sender<DaemonMessage>,
    logger: &L,
) -> crate::cli::Result<()> {
    let listener = UnixControl.bind(provider_name)?;
    let provider_name = provider_name.to_owned();
    let logger = logger.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let mut bytes = Vec::new();
            if std::io::Read::read_to_end(&mut stream, &mut bytes).is_err() {
                continue;
            }
            let reply = control_reply_for_request(&bytes, &provider_name, &tx);
            let _ = std::io::Write::write_all(&mut stream, &reply.encode());
            if matches!(reply, ControlMessage::Ack) {
                break;
            }
        }
        logger.info(format!(
            "control server stopped for provider {provider_name}"
        ));
    });
    Ok(())
}

#[cfg(target_os = "windows")]
fn spawn_control_server<L: Logger + 'static>(
    provider_name: &str,
    tx: mpsc::Sender<DaemonMessage>,
    logger: &L,
) -> crate::cli::Result<()> {
    let listener = WindowsControl.bind(provider_name)?;
    let provider_name = provider_name.to_owned();
    let logger = logger.clone();
    std::thread::spawn(move || {
        loop {
            let stop = match listener.serve_one_exchange(&provider_name, |bytes| {
                let reply = control_reply_for_request(bytes, &provider_name, &tx);
                let stop = matches!(reply, ControlMessage::Ack);
                (reply, stop)
            }) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if stop {
                break;
            }
        }
        logger.info(format!(
            "control server stopped for provider {provider_name}"
        ));
    });
    Ok(())
}

#[cfg(not(any(unix, target_os = "windows")))]
fn spawn_control_server<L: Logger>(
    _provider_name: &str,
    _tx: mpsc::Sender<DaemonMessage>,
    _logger: &L,
) -> crate::cli::Result<()> {
    Err(crate::cli::Error::Validation(
        "named provider control transport is not supported on this platform".to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoOpLogger;

    #[derive(Debug, Clone, Copy, Default)]
    struct FailingProvideRunner;

    impl ProvideRunner for FailingProvideRunner {
        fn run<L: Logger + 'static>(
            &self,
            _command: &ProvideCommand,
            _logger: &L,
        ) -> crate::cli::Result<()> {
            Err(crate::cli::Error::Providers(
                crate::providers::Error::NotSupported,
            ))
        }
    }

    #[test]
    fn provide_returns_error_when_provider_startup_fails() {
        let command = ProvideCommand {
            name: Some("demo".to_owned()),
            path: None,
            config_dir: None,
            storage: None,
        };

        let err = command
            .run_with(&FailingProvideRunner)
            .expect_err("startup should fail");
        assert!(matches!(
            err,
            crate::cli::Error::Providers(crate::providers::Error::NotSupported)
        ));
    }

    #[test]
    fn default_runner_can_be_called_with_injected_logger() {
        let command = ProvideCommand {
            name: Some("demo".to_owned()),
            path: None,
            config_dir: None,
            storage: None,
        };
        let logger = NoOpLogger;
        let err = DefaultProvideRunner
            .run(&command, &logger)
            .expect_err("named provider is not configured in test");
        assert!(matches!(
            err,
            crate::cli::Error::Config(crate::config::Error::Read { .. })
        ));
    }

    #[test]
    fn resolve_inline_request_builds_single_provider_config() {
        let command = ProvideCommand {
            name: None,
            path: Some(PathBuf::from("/tmp/demo")),
            config_dir: None,
            storage: Some(ProvideStorageSubcommand::Local(LocalStorageArgs {
                root: PathBuf::from("/data/demo"),
            })),
        };

        let request = command
            .resolve_request()
            .expect("inline request should resolve");
        assert!(request.provider_name.is_none());
        assert_eq!(request.config.providers.len(), 1);
    }

    #[test]
    fn resolve_request_requires_name_or_path() {
        let command = ProvideCommand {
            name: None,
            path: None,
            config_dir: None,
            storage: None,
        };

        let err = command
            .resolve_request()
            .expect_err("request without target should fail");
        assert!(matches!(err, crate::cli::Error::MissingProvideTarget));
    }

    #[test]
    fn resolve_request_requires_storage_for_inline_path() {
        let command = ProvideCommand {
            name: None,
            path: Some(PathBuf::from("/tmp/demo")),
            config_dir: None,
            storage: None,
        };

        let err = command
            .resolve_request()
            .expect_err("inline path without storage should fail");
        assert!(matches!(err, crate::cli::Error::MissingProvideTarget));
    }
}
