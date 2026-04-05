use crate::application::connect::{
    Application as ConnectApplication, ConnectRepository, ConnectUseCase, Error as ConnectError,
    ServiceControl, ServiceLauncher,
};
use crate::cli::commands::config::{AddLikeArgs, resolve_temp_driver_spec_from_add_like_args};
use crate::cli::commands::connect_sync::{
    single_external_subcommand_name, ConnectSyncTempSubcommand,
};
use crate::config::ConfigDir;
use crate::domain::driver::{DriverConfig, StorageConfig};
use crate::{Logger, TracingLogger};
use clap::{Args, Subcommand};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Subcommand, Debug, Clone)]
pub enum ConnectSubcommand {
    /// Run a temporary (non-configured) driver.
    Temp(ConnectSyncTempSubcommand),
    /// Connect a configured driver by name (single token).
    #[command(external_subcommand)]
    Named(Vec<String>),
}

/// Connect command ensures configured driver processes are running.
#[derive(Args, Debug, Clone)]
pub struct ConnectCommand {
    /// Connect all configured drivers.
    #[arg(long)]
    pub all: bool,

    #[command(subcommand)]
    pub action: Option<ConnectSubcommand>,

    /// Config directory override.
    #[arg(long)]
    pub config_dir: Option<PathBuf>,
}

impl ConnectCommand {
    pub fn execute(&self) -> crate::cli::Result<()> {
        let logger = TracingLogger::new();
        let config_dir = self.config_dir();
        self.validate_arguments()?;

        if let Some(ConnectSubcommand::Temp(temp)) = &self.action {
            let spec = resolve_temp_spec_from_subcommand(temp)?;
            let mut child = spawn_temp_driver_process(&spec, config_dir.dir())?;
            wait_until_ready(&spec.name, &mut child, &logger)?;
            return Ok(());
        }

        let repository = ConfigRepository::new(config_dir.clone());
        let control = ProviderServiceControl;
        let launcher = ProcessServiceLauncher::new(logger);
        let app = ConnectApplication::new(config_dir.dir(), &repository, &control, &launcher);
        self._execute(&app)
    }

    pub(crate) fn _execute<U>(&self, use_case: &U) -> crate::cli::Result<()>
    where
        U: ConnectUseCase,
    {
        self.validate_arguments()?;
        if self.all {
            use_case.connect_all().map_err(map_connect_error)
        } else {
            match &self.action {
                None => Err(crate::cli::Error::MissingConnectTarget),
                Some(ConnectSubcommand::Temp(_)) => Err(crate::cli::Error::Validation(
                    "temp is only supported via connect execute()".to_owned(),
                )),
                Some(ConnectSubcommand::Named(tokens)) => {
                    let name = single_external_subcommand_name(
                        tokens,
                        crate::cli::Error::MissingConnectTarget,
                    )?;
                    use_case.connect_name(name).map_err(map_connect_error)
                }
            }
        }
    }

    fn config_dir(&self) -> ConfigDir {
        match &self.config_dir {
            Some(p) => ConfigDir::new(p.clone()),
            None => ConfigDir::default(),
        }
    }

    fn validate_arguments(&self) -> crate::cli::Result<()> {
        if self.all {
            if self.action.is_some() {
                return Err(crate::cli::Error::Validation(
                    "--all cannot be combined with a subcommand".to_owned(),
                ));
            }
            return Ok(());
        }

        Ok(())
    }
}

fn resolve_temp_spec_from_subcommand(
    cmd: &ConnectSyncTempSubcommand,
) -> crate::cli::Result<DriverConfig> {
    resolve_temp_driver_spec_from_add_like_args(&AddLikeArgs {
        name: None,
        path: Some(cmd.path.clone()),
        storage: Some(cmd.storage.clone()),
    })
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

impl ConnectRepository for ConfigRepository {
    fn list_names(&self) -> crate::application::connect::Result<Vec<String>> {
        self.config_dir.list().map_err(Into::into)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ProviderServiceControl;

impl ServiceControl for ProviderServiceControl {
    fn ready(&self, driver_name: &str) -> bool {
        crate::cli::provider_control::provider_daemon_ready(driver_name)
    }
}

#[derive(Debug, Clone)]
struct ProcessServiceLauncher<L: Logger> {
    logger: L,
}

impl<L: Logger> ProcessServiceLauncher<L> {
    fn new(logger: L) -> Self {
        Self { logger }
    }
}

impl<L: Logger> ServiceLauncher for ProcessServiceLauncher<L> {
    fn launch(&self, driver_name: &str, config_dir: &Path) -> std::result::Result<(), String> {
        let mut child =
            spawn_named_driver_process(driver_name, config_dir).map_err(|error| error.to_string())?;
        wait_until_ready(driver_name, &mut child, &self.logger).map_err(|error| error.to_string())
    }
}

fn map_connect_error(error: ConnectError) -> crate::cli::Error {
    match error {
        ConnectError::Config(source) => crate::cli::Error::Config(source),
        ConnectError::ConnectFailures { failures } => {
            crate::cli::Error::ConnectFailures { failures }
        }
        ConnectError::Launch {
            driver_name,
            reason,
        } => crate::cli::Error::Validation(format!("{driver_name}: {reason}")),
    }
}

fn spawn_named_driver_process(
    driver_name: &str,
    config_dir: &Path,
) -> crate::cli::Result<std::process::Child> {
    let current_exe = std::env::current_exe()
        .map_err(|source| crate::cli::Error::ResolveCurrentExecutable { source })?;
    Command::new(current_exe)
        .arg("connect-sync")
        .arg("--config-dir")
        .arg(config_dir)
        .arg(driver_name)
        .spawn()
        .map_err(|source| crate::cli::Error::SpawnDriver {
            driver_name: driver_name.to_owned(),
            source,
        })
}

fn spawn_temp_driver_process(
    spec: &DriverConfig,
    config_dir: &Path,
) -> crate::cli::Result<std::process::Child> {
    let current_exe = std::env::current_exe()
        .map_err(|source| crate::cli::Error::ResolveCurrentExecutable { source })?;
    let mut command = Command::new(current_exe);
    command.arg("connect-sync");
    command.arg("--config-dir").arg(config_dir);
    command.arg("temp").arg(&spec.path);

    match &spec.storage {
        StorageConfig::Local { root } => {
            command.arg("local").arg(root);
        }
        StorageConfig::OneDrive {
            root,
            endpoint,
            access_token,
            refresh_token,
            client_id,
            token_expiry_buffer_secs,
        } => {
            command.arg("onedrive");
            command.arg("--root").arg(root);
            command.arg("--endpoint").arg(endpoint);
            if let Some(token) = access_token {
                command.arg("--access-token").arg(token);
            }
            if let Some(token) = refresh_token {
                command.arg("--refresh-token").arg(token);
            }
            if let Some(id) = client_id {
                command.arg("--client-id").arg(id);
            }
            if let Some(secs) = token_expiry_buffer_secs {
                command
                    .arg("--token-expiry-buffer-secs")
                    .arg(secs.to_string());
            }
        }
    }

    command.spawn().map_err(|source| crate::cli::Error::SpawnDriver {
        driver_name: spec.name.clone(),
        source,
    })
}

/// Poll frequently enough to make `connect` feel immediate while still giving
/// the spawned process time to bind its control endpoint and mount storage.
const READY_TIMEOUT: Duration = Duration::from_secs(5);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(50);

fn wait_until_ready<L: Logger>(
    driver_name: &str,
    child: &mut std::process::Child,
    logger: &L,
) -> crate::cli::Result<()> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        let child_status = child
            .try_wait()
            .map(|status| status.map(|value| value.to_string()))
            .map_err(|source| crate::cli::Error::WaitForDriver {
                driver_name: driver_name.to_owned(),
                source,
            })?;

        match next_ready_action(
            driver_name,
            crate::cli::provider_control::provider_daemon_ready(driver_name),
            child_status,
            Instant::now() >= deadline,
        ) {
            Ok(ReadyAction::Ready) => {
                logger.info(format!("Driver {driver_name} is ready"));
                return Ok(());
            }
            Ok(ReadyAction::Wait) => {}
            Err(error) => return Err(error),
        }

        thread::sleep(READY_POLL_INTERVAL);
    }
}

#[derive(Debug)]
enum ReadyAction {
    Ready,
    Wait,
}

fn next_ready_action(
    driver_name: &str,
    is_running: bool,
    child_status: Option<String>,
    deadline_expired: bool,
) -> crate::cli::Result<ReadyAction> {
    if is_running {
        return Ok(ReadyAction::Ready);
    }

    if let Some(status) = child_status {
        return Err(crate::cli::Error::DriverExitedBeforeReady {
            driver_name: driver_name.to_owned(),
            status,
        });
    }

    if deadline_expired {
        return Err(crate::cli::Error::DriverDidNotBecomeReady {
            driver_name: driver_name.to_owned(),
        });
    }

    Ok(ReadyAction::Wait)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::connect::{ConnectUseCase, Error as ConnectError};
    use crate::cli::commands::connect_sync::{ConnectSyncStorageSubcommand, LocalStorageArgs};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingUseCase {
        calls: Arc<Mutex<Vec<String>>>,
        connect_name_errors: HashMap<String, String>,
        connect_all_error: Option<String>,
    }

    impl RecordingUseCase {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().expect("calls lock").clone()
        }

        fn with_name_error(mut self, provider_name: &str, reason: &str) -> Self {
            self.connect_name_errors
                .insert(provider_name.to_owned(), reason.to_owned());
            self
        }

        fn with_all_error(mut self, failures: &str) -> Self {
            self.connect_all_error = Some(failures.to_owned());
            self
        }
    }

    impl ConnectUseCase for RecordingUseCase {
        fn connect_name(&self, driver_name: &str) -> crate::application::connect::Result<()> {
            self.calls
                .lock()
                .expect("calls lock")
                .push(format!("name:{driver_name}"));
            match self.connect_name_errors.get(driver_name) {
                Some(reason) => Err(ConnectError::Launch {
                    driver_name: driver_name.to_owned(),
                    reason: reason.clone(),
                }),
                None => Ok(()),
            }
        }

        fn connect_all(&self) -> crate::application::connect::Result<()> {
            self.calls
                .lock()
                .expect("calls lock")
                .push("all".to_owned());
            match &self.connect_all_error {
                Some(failures) => Err(ConnectError::ConnectFailures {
                    failures: failures.clone(),
                }),
                None => Ok(()),
            }
        }
    }

    #[test]
    fn execute_from_config_name() {
        let cmd = ConnectCommand {
            all: false,
            action: Some(ConnectSubcommand::Named(vec!["test".to_owned()])),
            config_dir: None,
        };

        let use_case = RecordingUseCase::default();
        cmd._execute(&use_case).expect("connect should succeed");

        assert_eq!(use_case.calls(), vec!["name:test"]);
    }

    #[test]
    fn execute_all_uses_application_use_case() {
        let cmd = ConnectCommand {
            all: true,
            action: None,
            config_dir: None,
        };

        let use_case = RecordingUseCase::default();
        cmd._execute(&use_case).expect("connect should succeed");

        assert_eq!(use_case.calls(), vec!["all"]);
    }

    #[test]
    fn connect_without_args_returns_missing_target_error() {
        let cmd = ConnectCommand {
            all: false,
            action: None,
            config_dir: None,
        };

        let err = cmd
            ._execute(&RecordingUseCase::default())
            .expect_err("connect should fail");
        assert!(matches!(err, crate::cli::Error::MissingConnectTarget));
    }

    #[test]
    fn execute_maps_application_failure_for_all() {
        let cmd = ConnectCommand {
            all: true,
            action: None,
            config_dir: None,
        };

        let err = cmd
            ._execute(&RecordingUseCase::default().with_all_error("broken: spawn failed"))
            .expect_err("connect all should fail");

        assert!(matches!(err, crate::cli::Error::ConnectFailures { .. }));
    }

    #[test]
    fn execute_maps_application_failure_for_name() {
        let cmd = ConnectCommand {
            all: false,
            action: Some(ConnectSubcommand::Named(vec!["demo".to_owned()])),
            config_dir: None,
        };

        let err = cmd
            ._execute(&RecordingUseCase::default().with_name_error("demo", "spawn failed"))
            .expect_err("connect should fail");

        assert!(matches!(err, crate::cli::Error::Validation(_)));
    }

    #[test]
    fn all_with_subcommand_fails_validation() {
        let cmd = ConnectCommand {
            all: true,
            action: Some(ConnectSubcommand::Named(vec!["demo".to_owned()])),
            config_dir: None,
        };

        let err = cmd
            ._execute(&RecordingUseCase::default())
            .expect_err("all with subcommand should fail");
        assert!(matches!(err, crate::cli::Error::Validation(_)));
    }

    #[test]
    fn named_external_rejects_multiple_tokens() {
        let cmd = ConnectCommand {
            all: false,
            action: Some(ConnectSubcommand::Named(vec![
                "a".to_owned(),
                "b".to_owned(),
            ])),
            config_dir: None,
        };

        let err = cmd
            ._execute(&RecordingUseCase::default())
            .expect_err("multiple name tokens should fail");
        assert!(matches!(err, crate::cli::Error::Validation(_)));
    }

    #[test]
    fn temp_subcommand_is_not_routed_through_execute_injected_tests() {
        let cmd = ConnectCommand {
            all: false,
            action: Some(ConnectSubcommand::Temp(ConnectSyncTempSubcommand {
                path: PathBuf::from("/mnt/demo"),
                storage: ConnectSyncStorageSubcommand::Local(LocalStorageArgs {
                    root: PathBuf::from("/data/demo"),
                }),
            })),
            config_dir: None,
        };

        let err = cmd
            ._execute(&RecordingUseCase::default())
            .expect_err("temp should not use _execute");
        assert!(matches!(err, crate::cli::Error::Validation(_)));
    }

    #[test]
    fn ready_check_reports_exited_process_before_ready() {
        let outcome = next_ready_action("demo", false, Some("exit status 1".to_owned()), false)
            .expect_err("exited child should fail");

        assert!(matches!(
            outcome,
            crate::cli::Error::DriverExitedBeforeReady { .. }
        ));
    }

    #[test]
    fn ready_check_reports_timeout_when_deadline_passes() {
        let outcome =
            next_ready_action("demo", false, None, true).expect_err("expired deadline should fail");

        assert!(matches!(
            outcome,
            crate::cli::Error::DriverDidNotBecomeReady { .. }
        ));
    }
}
