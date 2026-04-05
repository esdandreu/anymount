use crate::application::provide::{
    Application as ProvideApplication, DriverRuntimeHost, Error as ProvideError, ProvideRepository,
    ProvideUseCase, TelemetryFactory,
};
use crate::application::types::ProvideRequest;
use crate::cli::commands::config::{AddLikeArgs, resolve_temp_driver_spec_from_add_like_args};
use crate::config::ConfigDir;
use crate::domain::driver::{DriverConfig, StorageConfig};
use crate::drivers::Session;
use crate::service::ServiceRuntime;
use crate::service::control::messages::{ControlMessage, ServiceMessage};
use crate::{Logger, TracingLogger};
use clap::{Args, Subcommand};
use std::path::PathBuf;
use std::sync::mpsc;

#[cfg(unix)]
use crate::service::control::unix::UnixControl;

#[cfg(target_os = "windows")]
use crate::service::control::windows::WindowsControl;

/// Mount path plus storage for `connect temp` / `connect-sync temp`.
#[derive(Args, Debug, Clone)]
pub struct ConnectSyncTempSubcommand {
    /// Mount point for the temporary driver.
    #[arg(value_name = "PATH")]
    pub path: PathBuf,

    #[command(subcommand)]
    pub storage: ConnectSyncStorageSubcommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConnectSyncSubcommand {
    /// Run a temporary (non-configured) driver.
    Temp(ConnectSyncTempSubcommand),
    /// Run a configured driver by name (single token).
    #[command(external_subcommand)]
    Named(Vec<String>),
}

#[derive(Args, Debug, Clone)]
pub struct ConnectSyncCommand {
    #[arg(long)]
    pub config_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub action: ConnectSyncSubcommand,
}

pub(crate) fn single_external_subcommand_name(
    args: &[String],
    empty_error: crate::cli::Error,
) -> crate::cli::Result<&str> {
    match args {
        [one] => Ok(one.as_str()),
        [] => Err(empty_error),
        _ => Err(crate::cli::Error::Validation(
            "expected exactly one configuration NAME".to_owned(),
        )),
    }
}

impl ConnectSyncCommand {
    pub fn execute(&self) -> crate::cli::Result<()> {
        let logger = TracingLogger::new();
        let config_dir = self.config_dir();
        let repository = ConfigRepository::new(config_dir);
        let telemetry = DefaultTelemetryFactory;
        let host = RuntimeHost::new(logger);
        let app = ProvideApplication::new(&repository, &telemetry, &host);
        self.run_with(&app)
    }

    pub(crate) fn run_with<U>(&self, use_case: &U) -> crate::cli::Result<()>
    where
        U: ProvideUseCase,
    {
        match &self.action {
            ConnectSyncSubcommand::Temp(args) => {
                let spec = resolve_temp_driver_spec_from_add_like_args(&AddLikeArgs {
                    name: None,
                    path: Some(args.path.clone()),
                    storage: Some(args.storage.clone()),
                })?;
                let control_name = spec.name.clone();
                use_case
                    .run_inline_with_control(spec, control_name)
                    .map_err(map_connect_sync_error)
            }
            ConnectSyncSubcommand::Named(tokens) => {
                let name = single_external_subcommand_name(
                    tokens,
                    crate::cli::Error::MissingConnectSyncTarget,
                )?;
                use_case.run_named(name).map_err(map_connect_sync_error)
            }
        }
    }

    fn config_dir(&self) -> ConfigDir {
        match &self.config_dir {
            Some(path) => ConfigDir::new(path.clone()),
            None => ConfigDir::default(),
        }
    }
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConnectSyncStorageSubcommand {
    Local(LocalStorageArgs),
    #[command(name = "onedrive")]
    OneDrive(OneDriveStorageArgs),
}

#[derive(Args, Debug, Clone)]
pub struct LocalStorageArgs {
    #[arg(value_name = "ROOT")]
    pub root: PathBuf,
}

#[derive(Args, Debug, Clone)]
pub struct OneDriveStorageArgs {
    #[arg(long, default_value = "/", value_name = "PATH")]
    pub root: PathBuf,
    #[arg(
        long,
        default_value = "https://graph.microsoft.com/v1.0",
        value_name = "URL"
    )]
    pub endpoint: String,
    #[arg(long, value_name = "TOKEN")]
    pub access_token: Option<String>,
    #[arg(long, value_name = "TOKEN")]
    pub refresh_token: Option<String>,
    #[arg(long, value_name = "ID")]
    pub client_id: Option<String>,
    #[arg(long, default_value = "60", value_name = "SECS")]
    pub token_expiry_buffer_secs: u64,
}

impl ConnectSyncStorageSubcommand {
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

#[derive(Debug, Clone)]
struct ConfigRepository {
    config_dir: ConfigDir,
}

impl ConfigRepository {
    fn new(config_dir: ConfigDir) -> Self {
        Self { config_dir }
    }
}

impl ProvideRepository for ConfigRepository {
    fn read_spec(&self, name: &str) -> crate::application::provide::Result<DriverConfig> {
        self.config_dir.read_spec(name).map_err(Into::into)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct DefaultTelemetryFactory;

impl TelemetryFactory for DefaultTelemetryFactory {
    fn build(
        &self,
        spec: &DriverConfig,
    ) -> crate::application::provide::Result<Option<crate::telemetry::OtelHandles>> {
        crate::telemetry::OtelHandles::from_driver_spec(spec).map_err(Into::into)
    }
}

#[derive(Debug, Clone)]
struct RuntimeHost<L: Logger> {
    logger: L,
}

impl<L: Logger> RuntimeHost<L> {
    fn new(logger: L) -> Self {
        Self { logger }
    }
}

impl<L: Logger + 'static> DriverRuntimeHost for RuntimeHost<L> {
    fn run(
        &self,
        request: ProvideRequest,
        telemetry: Option<crate::telemetry::OtelHandles>,
    ) -> crate::application::provide::Result<()> {
        let driver_name = request
            .control_name
            .clone()
            .unwrap_or_else(|| request.spec.name.clone());

        let result = (|| {
            let (tx, rx) = mpsc::channel();
            install_ctrlc_handler(tx.clone(), &self.logger).map_err(|error| {
                ProvideError::Host {
                    driver_name: driver_name.clone(),
                    reason: error.to_string(),
                }
            })?;

            let drivers: Vec<Box<dyn Session>> = crate::connect_drivers_with_telemetry(
                std::slice::from_ref(&request.spec),
                &self.logger,
                Some(tx.clone()),
            )
            .map_err(|error| ProvideError::Host {
                driver_name: driver_name.clone(),
                reason: error.to_string(),
            })?;

            for driver in &drivers {
                self.logger.info(format!(
                    "Connected to {} at {}",
                    driver.kind(),
                    driver.path().display()
                ));
            }

            if let Some(control_name) = request.control_name.as_deref() {
                spawn_control_server(control_name, tx, &self.logger).map_err(|error| {
                    ProvideError::Host {
                        driver_name: control_name.to_owned(),
                        reason: error.to_string(),
                    }
                })?;
            }

            let mut runtime = ServiceRuntime::new(self.logger.clone(), rx);
            let result = runtime.run().map_err(|error| ProvideError::Host {
                driver_name: driver_name.clone(),
                reason: error.to_string(),
            });
            drop(drivers);
            result
        })();

        if let Some(otel) = telemetry {
            otel.shutdown();
        }

        result
    }
}

fn map_connect_sync_error(error: ProvideError) -> crate::cli::Error {
    match error {
        ProvideError::Config(source) => crate::cli::Error::Config(source),
        ProvideError::Telemetry(source) => crate::cli::Error::Otlp(source),
        ProvideError::Repository {
            driver_name,
            reason,
        }
        | ProvideError::Host {
            driver_name,
            reason,
        } => crate::cli::Error::Validation(format!("{driver_name}: {reason}")),
    }
}

fn control_reply_for_request(
    bytes: &[u8],
    driver_name: &str,
    tx: &mpsc::Sender<ServiceMessage>,
) -> ControlMessage {
    match ControlMessage::decode(bytes) {
        Ok(ControlMessage::Ping) => ControlMessage::Ready,
        Ok(ControlMessage::Shutdown) => {
            let _ = tx.send(ServiceMessage::Shutdown);
            ControlMessage::Ack
        }
        Ok(other) => ControlMessage::Error(format!(
            "unsupported control message for {driver_name}: {other:?}"
        )),
        Err(error) => ControlMessage::Error(error.to_string()),
    }
}

fn install_ctrlc_handler<L: Logger>(
    tx: mpsc::Sender<ServiceMessage>,
    logger: &L,
) -> crate::cli::Result<()> {
    ctrlc::set_handler(move || {
        let _ = tx.send(ServiceMessage::Shutdown);
    })
    .map_err(|source| {
        logger.error(format!("Error setting Ctrl-C handler: {source}"));
        crate::cli::Error::InstallCtrlC { source }
    })
}

#[cfg(unix)]
fn spawn_control_server<L: Logger + 'static>(
    driver_name: &str,
    tx: mpsc::Sender<ServiceMessage>,
    logger: &L,
) -> crate::cli::Result<()> {
    let listener = UnixControl.bind(driver_name)?;
    let driver_name = driver_name.to_owned();
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
            let reply = control_reply_for_request(&bytes, &driver_name, &tx);
            let _ = std::io::Write::write_all(&mut stream, &reply.encode());
            if matches!(reply, ControlMessage::Ack) {
                break;
            }
        }
        logger.info(format!("control server stopped for driver {driver_name}"));
    });
    Ok(())
}

#[cfg(target_os = "windows")]
fn spawn_control_server<L: Logger + 'static>(
    driver_name: &str,
    tx: mpsc::Sender<ServiceMessage>,
    logger: &L,
) -> crate::cli::Result<()> {
    let listener = WindowsControl.bind(driver_name)?;
    let driver_name = driver_name.to_owned();
    let logger = logger.clone();
    std::thread::spawn(move || {
        loop {
            let stop = match listener.serve_one_exchange(&driver_name, |bytes| {
                let reply = control_reply_for_request(bytes, &driver_name, &tx);
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
        logger.info(format!("control server stopped for driver {driver_name}"));
    });
    Ok(())
}

#[cfg(not(any(unix, target_os = "windows")))]
fn spawn_control_server<L: Logger>(
    _driver_name: &str,
    _tx: mpsc::Sender<ServiceMessage>,
    _logger: &L,
) -> crate::cli::Result<()> {
    Err(crate::cli::Error::Validation(
        "named driver control transport is not supported on this platform".to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::provide::{Error as ProvideError, ProvideUseCase};

    #[derive(Default)]
    struct RecordingUseCase {
        named_calls: Vec<String>,
        inline_calls: Vec<String>,
        controlled_calls: Vec<String>,
        fail_named: Option<String>,
    }

    impl RecordingUseCase {
        fn with_named_failure(mut self, reason: &str) -> Self {
            self.fail_named = Some(reason.to_owned());
            self
        }
    }

    impl ProvideUseCase for std::cell::RefCell<RecordingUseCase> {
        fn run_named(&self, name: &str) -> crate::application::provide::Result<()> {
            let mut state = self.borrow_mut();
            state.named_calls.push(name.to_owned());
            if let Some(reason) = &state.fail_named {
                return Err(ProvideError::Repository {
                    driver_name: name.to_owned(),
                    reason: reason.clone(),
                });
            }
            Ok(())
        }

        fn run_inline(&self, spec: DriverConfig) -> crate::application::provide::Result<()> {
            self.borrow_mut().inline_calls.push(spec.name);
            Ok(())
        }

        fn run_inline_with_control(
            &self,
            spec: DriverConfig,
            control_name: String,
        ) -> crate::application::provide::Result<()> {
            let mut state = self.borrow_mut();
            state.inline_calls.push(spec.name);
            state.controlled_calls.push(control_name);
            Ok(())
        }
    }

    #[test]
    fn connect_sync_returns_error_when_driver_startup_fails() {
        let command = ConnectSyncCommand {
            config_dir: None,
            action: ConnectSyncSubcommand::Named(vec!["demo".to_owned()]),
        };

        let err = command
            .run_with(&std::cell::RefCell::new(
                RecordingUseCase::default().with_named_failure("startup failed"),
            ))
            .expect_err("startup should fail");
        assert!(matches!(err, crate::cli::Error::Validation(_)));
    }

    #[test]
    fn default_runner_can_be_called_with_injected_use_case() {
        let command = ConnectSyncCommand {
            config_dir: None,
            action: ConnectSyncSubcommand::Named(vec!["demo".to_owned()]),
        };

        let use_case = std::cell::RefCell::new(RecordingUseCase::default());
        command
            .run_with(&use_case)
            .expect("connect-sync should succeed");
        assert_eq!(use_case.borrow().named_calls, vec!["demo"]);
    }

    #[test]
    fn named_external_rejects_multiple_tokens() {
        let command = ConnectSyncCommand {
            config_dir: None,
            action: ConnectSyncSubcommand::Named(vec!["a".to_owned(), "b".to_owned()]),
        };

        let err = command
            .run_with(&std::cell::RefCell::new(RecordingUseCase::default()))
            .expect_err("command should fail");
        assert!(matches!(err, crate::cli::Error::Validation(_)));
    }

    #[test]
    fn resolve_temp_request_builds_single_driver_spec() {
        let command = ConnectSyncCommand {
            config_dir: None,
            action: ConnectSyncSubcommand::Temp(ConnectSyncTempSubcommand {
                path: PathBuf::from("/mnt/demo"),
                storage: ConnectSyncStorageSubcommand::Local(LocalStorageArgs {
                    root: PathBuf::from("/data/demo"),
                }),
            }),
        };

        let use_case = std::cell::RefCell::new(RecordingUseCase::default());
        command
            .run_with(&use_case)
            .expect("connect-sync should succeed");
        assert_eq!(use_case.borrow().inline_calls, vec!["temp"]);
        assert_eq!(use_case.borrow().controlled_calls, vec!["temp"]);
    }
}
