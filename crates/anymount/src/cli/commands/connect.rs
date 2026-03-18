use crate::Logger;
use crate::TracingLogger;
use crate::config::ConfigDir;
use crate::daemon::messages::ControlMessage;
use clap::Args;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use crate::daemon::control_unix::UnixControl;

#[cfg(target_os = "windows")]
use crate::daemon::control_windows::WindowsControl;

/// Connect command ensures configured provider processes are running.
#[derive(Args, Debug, Clone)]
pub struct ConnectCommand {
    /// Connect a named provider from config.
    #[arg(long, conflicts_with = "all")]
    pub name: Option<String>,

    /// Connect all configured providers.
    #[arg(long, conflicts_with = "name")]
    pub all: bool,

    /// Config directory override.
    #[arg(long)]
    pub config_dir: Option<PathBuf>,
}

impl ConnectCommand {
    pub fn execute(&self) -> crate::cli::Result<()> {
        let logger = TracingLogger::new();
        self._execute(&DefaultProviderProcessSupervisor, &logger)
    }

    pub(crate) fn _execute<S, L>(&self, supervisor: &S, logger: &L) -> crate::cli::Result<()>
    where
        S: ProviderProcessSupervisor,
        L: Logger + 'static,
    {
        if self.all {
            let cd = self.config_dir();
            let mut failures = Vec::new();
            for name in cd.list()? {
                if let Err(error) = supervisor.ensure_running(&name, &cd, logger) {
                    failures.push(format!("{name}: {error}"));
                }
            }
            if failures.is_empty() {
                Ok(())
            } else {
                Err(crate::cli::Error::ConnectFailures {
                    failures: failures.join(", "),
                })
            }
        } else if let Some(name) = &self.name {
            let cd = self.config_dir();
            supervisor.ensure_running(name, &cd, logger)
        } else {
            Err(crate::cli::Error::MissingConnectTarget)
        }
    }

    fn config_dir(&self) -> ConfigDir {
        match &self.config_dir {
            Some(p) => ConfigDir::new(p.clone()),
            None => ConfigDir::default(),
        }
    }
}

pub trait ProviderProcessSupervisor {
    fn ensure_running<L>(
        &self,
        provider_name: &str,
        config_dir: &ConfigDir,
        logger: &L,
    ) -> crate::cli::Result<()>
    where
        L: Logger + 'static;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultProviderProcessSupervisor;

impl ProviderProcessSupervisor for DefaultProviderProcessSupervisor {
    fn ensure_running<L>(
        &self,
        provider_name: &str,
        config_dir: &ConfigDir,
        logger: &L,
    ) -> crate::cli::Result<()>
    where
        L: Logger + 'static,
    {
        if is_provider_running(provider_name)? {
            logger.info(format!("Provider {provider_name} is already running"));
            return Ok(());
        }

        let mut child = spawn_provider_process(provider_name, config_dir.dir())?;
        wait_until_ready(provider_name, &mut child, logger)
    }
}

#[cfg(unix)]
fn is_provider_running(provider_name: &str) -> crate::cli::Result<bool> {
    match UnixControl.send(provider_name, ControlMessage::Ping) {
        Ok(ControlMessage::Ready) => Ok(true),
        Ok(_) => Ok(false),
        Err(_) => Ok(false),
    }
}

#[cfg(target_os = "windows")]
fn is_provider_running(provider_name: &str) -> crate::cli::Result<bool> {
    match WindowsControl.send(provider_name, ControlMessage::Ping) {
        Ok(ControlMessage::Ready) => Ok(true),
        Ok(_) => Ok(false),
        Err(_) => Ok(false),
    }
}

#[cfg(not(any(unix, target_os = "windows")))]
fn is_provider_running(_provider_name: &str) -> crate::cli::Result<bool> {
    Ok(false)
}

fn spawn_provider_process(
    provider_name: &str,
    config_dir: &Path,
) -> crate::cli::Result<std::process::Child> {
    let current_exe = std::env::current_exe()
        .map_err(|source| crate::cli::Error::ResolveCurrentExecutable { source })?;
    Command::new(current_exe)
        .arg("provide")
        .arg("--name")
        .arg(provider_name)
        .arg("--config-dir")
        .arg(config_dir)
        .spawn()
        .map_err(|source| crate::cli::Error::SpawnProvider {
            provider_name: provider_name.to_owned(),
            source,
        })
}

/// Poll frequently enough to make `connect` feel immediate while still giving
/// the spawned process time to bind its control endpoint and mount storage.
const READY_TIMEOUT: Duration = Duration::from_secs(5);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(50);

fn wait_until_ready<L: Logger>(
    provider_name: &str,
    child: &mut std::process::Child,
    logger: &L,
) -> crate::cli::Result<()> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        let child_status = child
            .try_wait()
            .map(|status| status.map(|value| value.to_string()))
            .map_err(|source| crate::cli::Error::WaitForProvider {
                provider_name: provider_name.to_owned(),
                source,
            })?;

        match next_ready_action(
            provider_name,
            is_provider_running(provider_name)?,
            child_status,
            Instant::now() >= deadline,
        ) {
            Ok(ReadyAction::Ready) => {
                logger.info(format!("Provider {provider_name} is ready"));
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
    provider_name: &str,
    is_running: bool,
    child_status: Option<String>,
    deadline_expired: bool,
) -> crate::cli::Result<ReadyAction> {
    if is_running {
        return Ok(ReadyAction::Ready);
    }

    if let Some(status) = child_status {
        return Err(crate::cli::Error::ProviderExitedBeforeReady {
            provider_name: provider_name.to_owned(),
            status,
        });
    }

    if deadline_expired {
        return Err(crate::cli::Error::ProviderDidNotBecomeReady {
            provider_name: provider_name.to_owned(),
        });
    }

    Ok(ReadyAction::Wait)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoOpLogger;
    use crate::{ProviderFileConfig, StorageConfig};
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct RecordingSupervisor {
        running: Arc<Mutex<HashSet<String>>>,
        failures: Arc<Mutex<HashMap<String, String>>>,
        ensured: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingSupervisor {
        fn with_running(provider_name: &str) -> Self {
            let supervisor = Self::default();
            supervisor
                .running
                .lock()
                .expect("running providers lock should not be poisoned")
                .insert(provider_name.to_owned());
            supervisor
        }

        fn with_failure(provider_name: &str, error: &str) -> Self {
            let supervisor = Self::default();
            supervisor
                .failures
                .lock()
                .expect("failure map lock should not be poisoned")
                .insert(provider_name.to_owned(), error.to_owned());
            supervisor
        }

        fn ensured(&self) -> Vec<String> {
            self.ensured
                .lock()
                .expect("ensure log lock should not be poisoned")
                .clone()
        }
    }

    impl ProviderProcessSupervisor for RecordingSupervisor {
        fn ensure_running<L>(
            &self,
            provider_name: &str,
            _config_dir: &ConfigDir,
            _logger: &L,
        ) -> crate::cli::Result<()>
        where
            L: Logger + 'static,
        {
            self.ensured
                .lock()
                .expect("ensure log lock should not be poisoned")
                .push(provider_name.to_owned());

            if let Some(error) = self
                .failures
                .lock()
                .expect("failure map lock should not be poisoned")
                .get(provider_name)
                .cloned()
            {
                return Err(crate::cli::Error::Validation(error));
            }

            self.running
                .lock()
                .expect("running providers lock should not be poisoned")
                .insert(provider_name.to_owned());
            Ok(())
        }
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
            config_dir: Some(tmp.path().to_path_buf()),
        };
        let logger = NoOpLogger;
        let supervisor = RecordingSupervisor::default();
        let result = cmd._execute(&supervisor, &logger);
        assert!(result.is_ok());
        assert_eq!(supervisor.ensured(), vec!["test"]);
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
            config_dir: Some(tmp.path().to_path_buf()),
        };
        let logger = NoOpLogger;
        let supervisor = RecordingSupervisor::default();
        let result = cmd._execute(&supervisor, &logger);
        assert!(result.is_ok());
        assert_eq!(supervisor.ensured(), vec!["a"]);
    }

    #[test]
    fn connect_without_args_returns_missing_target_error() {
        let cmd = ConnectCommand {
            name: None,
            all: false,
            config_dir: None,
        };
        let logger = NoOpLogger;
        let err = cmd
            ._execute(&RecordingSupervisor::default(), &logger)
            .expect_err("connect should fail");
        assert!(matches!(err, crate::cli::Error::MissingConnectTarget));
    }

    #[test]
    fn execute_reuses_running_provider_daemon() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        cd.write(
            "demo",
            &crate::ProviderFileConfig {
                path: PathBuf::from("/mnt/demo"),
                storage: crate::StorageConfig::Local {
                    root: PathBuf::from("/data/demo"),
                },
            },
        )
        .expect("write failed");

        let cmd = ConnectCommand {
            name: Some("demo".to_owned()),
            all: false,
            config_dir: Some(tmp.path().to_path_buf()),
        };
        let supervisor = RecordingSupervisor::with_running("demo");
        cmd._execute(&supervisor, &NoOpLogger)
            .expect("connect should succeed");

        assert_eq!(supervisor.ensured(), vec!["demo"]);
    }

    #[test]
    fn execute_returns_error_when_one_provider_fails_during_all() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        for (name, path) in [("healthy", "/mnt/healthy"), ("broken", "/mnt/broken")] {
            cd.write(
                name,
                &crate::ProviderFileConfig {
                    path: PathBuf::from(path),
                    storage: crate::StorageConfig::Local {
                        root: PathBuf::from(format!("/data/{name}")),
                    },
                },
            )
            .expect("write failed");
        }

        let cmd = ConnectCommand {
            name: None,
            all: true,
            config_dir: Some(tmp.path().to_path_buf()),
        };
        let supervisor = RecordingSupervisor::with_failure("broken", "startup failed");

        let err = cmd
            ._execute(&supervisor, &NoOpLogger)
            .expect_err("connect all should fail");

        assert!(matches!(err, crate::cli::Error::ConnectFailures { .. }));
        assert!(supervisor.ensured().contains(&"healthy".to_owned()));
    }

    #[test]
    fn ready_check_reports_exited_process_before_ready() {
        let outcome = next_ready_action("demo", false, Some("exit status 1".to_owned()), false)
            .expect_err("exited child should fail");

        assert!(matches!(
            outcome,
            crate::cli::Error::ProviderExitedBeforeReady { .. }
        ));
    }

    #[test]
    fn ready_check_reports_timeout_when_deadline_passes() {
        let outcome =
            next_ready_action("demo", false, None, true).expect_err("expired deadline should fail");

        assert!(matches!(
            outcome,
            crate::cli::Error::ProviderDidNotBecomeReady { .. }
        ));
    }
}
