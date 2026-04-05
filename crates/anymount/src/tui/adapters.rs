use super::{Error, Result};
use crate::Logger;
use crate::application::config::ConfigRepository;
use crate::application::connect::{ConnectRepository, ServiceControl, ServiceLauncher};
use crate::config::ConfigDir;
use crate::domain::driver::DriverConfig;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub(crate) struct TuiConfigRepository {
    config_dir: ConfigDir,
}

impl TuiConfigRepository {
    pub(crate) fn new(config_dir: ConfigDir) -> Self {
        Self { config_dir }
    }
}

impl ConfigRepository for TuiConfigRepository {
    fn list_names(&self) -> crate::application::config::Result<Vec<String>> {
        self.config_dir.list().map_err(Into::into)
    }

    fn read_spec(&self, name: &str) -> crate::application::config::Result<DriverConfig> {
        self.config_dir.read_spec(name).map_err(Into::into)
    }

    fn write_spec(&self, spec: &DriverConfig) -> crate::application::config::Result<()> {
        self.config_dir.write_spec(spec).map_err(Into::into)
    }

    fn remove(&self, name: &str) -> crate::application::config::Result<()> {
        self.config_dir.remove(name).map_err(Into::into)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TuiConnectRepository {
    config_dir: ConfigDir,
}

impl TuiConnectRepository {
    pub(crate) fn new(config_dir: ConfigDir) -> Self {
        Self { config_dir }
    }
}

impl ConnectRepository for TuiConnectRepository {
    fn list_names(&self) -> crate::application::connect::Result<Vec<String>> {
        self.config_dir.list().map_err(Into::into)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TuiServiceControl;

impl ServiceControl for TuiServiceControl {
    fn ready(&self, provider_name: &str) -> bool {
        crate::cli::provider_control::provider_daemon_ready(provider_name)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessServiceLauncher<L: Logger> {
    logger: L,
}

impl<L: Logger> ProcessServiceLauncher<L> {
    pub(crate) fn new(logger: L) -> Self {
        Self { logger }
    }
}

impl<L: Logger> ServiceLauncher for ProcessServiceLauncher<L> {
    fn launch(&self, provider_name: &str, config_dir: &Path) -> std::result::Result<(), String> {
        let mut child =
            spawn_provider_process(provider_name, config_dir).map_err(|error| error.to_string())?;
        wait_until_ready(provider_name, &mut child, &self.logger).map_err(|error| error.to_string())
    }
}

pub(crate) const READY_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const READY_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub(crate) fn spawn_provider_process(
    provider_name: &str,
    config_dir: &Path,
) -> Result<std::process::Child> {
    let current_exe = std::env::current_exe()
        .map_err(|source| crate::cli::Error::ResolveCurrentExecutable { source })?;
    Command::new(current_exe)
        .arg("connect-sync")
        .arg("--name")
        .arg(provider_name)
        .arg("--config-dir")
        .arg(config_dir)
        .spawn()
        .map_err(|source| crate::cli::Error::SpawnDriver {
            driver_name: provider_name.to_owned(),
            source,
        })
        .map_err(Error::from)
}

pub(crate) fn wait_until_ready<L: Logger>(
    provider_name: &str,
    child: &mut std::process::Child,
    logger: &L,
) -> Result<()> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        let child_status = child
            .try_wait()
            .map(|status| status.map(|value| value.to_string()))
            .map_err(|source| crate::cli::Error::WaitForDriver {
                driver_name: provider_name.to_owned(),
                source,
            })
            .map_err(Error::from)?;

        match next_ready_action(
            provider_name,
            crate::cli::provider_control::provider_daemon_ready(provider_name),
            child_status,
            Instant::now() >= deadline,
        )? {
            ReadyAction::Ready => {
                logger.info(format!("Provider {provider_name} is ready"));
                return Ok(());
            }
            ReadyAction::Wait => {}
        }

        thread::sleep(READY_POLL_INTERVAL);
    }
}

#[derive(Debug)]
pub(crate) enum ReadyAction {
    Ready,
    Wait,
}

pub(crate) fn next_ready_action(
    provider_name: &str,
    is_running: bool,
    child_status: Option<String>,
    deadline_expired: bool,
) -> Result<ReadyAction> {
    if is_running {
        return Ok(ReadyAction::Ready);
    }

    if let Some(status) = child_status {
        return Err(crate::cli::Error::DriverExitedBeforeReady {
            driver_name: provider_name.to_owned(),
            status,
        }
        .into());
    }

    if deadline_expired {
        return Err(crate::cli::Error::DriverDidNotBecomeReady {
            driver_name: provider_name.to_owned(),
        }
        .into());
    }

    Ok(ReadyAction::Wait)
}

#[cfg(test)]
mod tests {
    use super::super::model::ProviderEntry;
    use super::super::services::connect_selected_provider;
    use super::super::state::AppState;
    use crate::DriverFileConfig;
    use crate::application::connect::{ConnectUseCase, Result as ConnectApplicationResult};
    use crate::domain::driver::StorageConfig;
    use std::cell::RefCell;
    use std::path::PathBuf;

    fn local_provider(name: &str) -> ProviderEntry {
        ProviderEntry {
            name: name.to_owned(),
            config: DriverFileConfig {
                path: PathBuf::from(format!("/mnt/{name}")),
                storage: StorageConfig::Local {
                    root: PathBuf::from(format!("/data/{name}")),
                },
                telemetry: Default::default(),
            },
        }
    }

    #[derive(Default)]
    struct RecordingConnectApp {
        connected_names: RefCell<Vec<String>>,
    }

    impl ConnectUseCase for RecordingConnectApp {
        fn connect_name(&self, provider_name: &str) -> ConnectApplicationResult<()> {
            self.connected_names
                .borrow_mut()
                .push(provider_name.to_owned());
            Ok(())
        }

        fn connect_all(&self) -> ConnectApplicationResult<()> {
            Ok(())
        }
    }

    #[test]
    fn connect_selected_provider_uses_application_connect() {
        let state = AppState {
            providers: vec![local_provider("alpha")],
            selected: 0,
            hovered: 0,
            is_keyboard_mode: true,
            status: String::new(),
            mode: super::super::edit::UiMode::Browse,
        };
        let app = RecordingConnectApp::default();

        let connected = connect_selected_provider(&app, &state).expect("connect should work");

        assert_eq!(connected, Some("alpha".to_owned()));
        assert_eq!(
            app.connected_names.borrow().as_slice(),
            ["alpha".to_owned()]
        );
    }
}
