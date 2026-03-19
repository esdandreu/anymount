use crate::application::disconnect::{
    Application as DisconnectApplication, DisconnectRepository, DisconnectUseCase,
    Error as DisconnectError, ServiceControl,
};
use crate::config::ConfigDir;
use clap::Args;
use std::path::PathBuf;

/// Stop background provider services via the control endpoint (idempotent).
#[derive(Args, Debug, Clone)]
pub struct DisconnectCommand {
    /// Disconnect a named provider.
    #[arg(long, conflicts_with = "all")]
    pub name: Option<String>,

    /// Disconnect all configured providers.
    #[arg(long, conflicts_with = "name")]
    pub all: bool,

    /// Config directory override.
    #[arg(long)]
    pub config_dir: Option<PathBuf>,
}

impl DisconnectCommand {
    pub fn execute(&self) -> crate::cli::Result<()> {
        let config_dir = self.config_dir();
        let repository = ConfigRepository::new(config_dir);
        let control = ProviderServiceControl;
        let app = DisconnectApplication::new(&repository, &control);
        self._execute(&app)
    }

    pub(crate) fn _execute<U>(&self, use_case: &U) -> crate::cli::Result<()>
    where
        U: DisconnectUseCase,
    {
        if self.all {
            use_case.disconnect_all().map_err(map_disconnect_error)
        } else if let Some(name) = &self.name {
            use_case.disconnect_name(name).map_err(map_disconnect_error)
        } else {
            Err(crate::cli::Error::MissingDisconnectTarget)
        }
    }

    fn config_dir(&self) -> ConfigDir {
        match &self.config_dir {
            Some(p) => ConfigDir::new(p.clone()),
            None => ConfigDir::default(),
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

impl DisconnectRepository for ConfigRepository {
    fn list_names(&self) -> crate::application::disconnect::Result<Vec<String>> {
        self.config_dir.list().map_err(Into::into)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ProviderServiceControl;

impl ServiceControl for ProviderServiceControl {
    fn disconnect(&self, provider_name: &str) -> std::result::Result<(), String> {
        crate::cli::provider_control::try_disconnect_provider(provider_name)
    }
}

fn map_disconnect_error(error: DisconnectError) -> crate::cli::Error {
    match error {
        DisconnectError::Config(source) => crate::cli::Error::Config(source),
        DisconnectError::DisconnectFailures { failures } => {
            crate::cli::Error::DisconnectFailures { failures }
        }
        DisconnectError::Disconnect {
            provider_name,
            reason,
        } => crate::cli::Error::DisconnectFailures {
            failures: format!("{provider_name}: {reason}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::disconnect::{DisconnectUseCase, Error as DisconnectError};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingUseCase {
        calls: Arc<Mutex<Vec<String>>>,
        disconnect_name_errors: HashMap<String, String>,
        disconnect_all_error: Option<String>,
    }

    impl RecordingUseCase {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().expect("calls lock").clone()
        }

        fn with_name_error(mut self, provider_name: &str, reason: &str) -> Self {
            self.disconnect_name_errors
                .insert(provider_name.to_owned(), reason.to_owned());
            self
        }

        fn with_all_error(mut self, failures: &str) -> Self {
            self.disconnect_all_error = Some(failures.to_owned());
            self
        }
    }

    impl DisconnectUseCase for RecordingUseCase {
        fn disconnect_name(
            &self,
            provider_name: &str,
        ) -> crate::application::disconnect::Result<()> {
            self.calls
                .lock()
                .expect("calls lock")
                .push(format!("name:{provider_name}"));
            match self.disconnect_name_errors.get(provider_name) {
                Some(reason) => Err(DisconnectError::Disconnect {
                    provider_name: provider_name.to_owned(),
                    reason: reason.clone(),
                }),
                None => Ok(()),
            }
        }

        fn disconnect_all(&self) -> crate::application::disconnect::Result<()> {
            self.calls
                .lock()
                .expect("calls lock")
                .push("all".to_owned());
            match &self.disconnect_all_error {
                Some(failures) => Err(DisconnectError::DisconnectFailures {
                    failures: failures.clone(),
                }),
                None => Ok(()),
            }
        }
    }

    #[test]
    fn disconnect_all_uses_application_use_case() {
        let cmd = DisconnectCommand {
            name: None,
            all: true,
            config_dir: None,
        };

        let use_case = RecordingUseCase::default();
        cmd._execute(&use_case).expect("disconnect should succeed");

        assert_eq!(use_case.calls(), vec!["all"]);
    }

    #[test]
    fn disconnect_without_target_errors() {
        let cmd = DisconnectCommand {
            name: None,
            all: false,
            config_dir: None,
        };

        let err = cmd
            ._execute(&RecordingUseCase::default())
            .expect_err("missing target");
        assert!(matches!(err, crate::cli::Error::MissingDisconnectTarget));
    }

    #[test]
    fn disconnect_all_aggregates_failures() {
        let cmd = DisconnectCommand {
            name: None,
            all: true,
            config_dir: None,
        };

        let err = cmd
            ._execute(&RecordingUseCase::default().with_all_error("bad: nope"))
            .expect_err("partial failure");
        assert!(matches!(err, crate::cli::Error::DisconnectFailures { .. }));
    }

    #[test]
    fn disconnect_name_maps_single_failure() {
        let cmd = DisconnectCommand {
            name: Some("demo".to_owned()),
            all: false,
            config_dir: None,
        };

        let err = cmd
            ._execute(&RecordingUseCase::default().with_name_error("demo", "nope"))
            .expect_err("disconnect should fail");
        assert!(matches!(err, crate::cli::Error::DisconnectFailures { .. }));
    }
}
