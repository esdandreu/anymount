use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] crate::config::Error),

    #[error("failed to connect driver {driver_name}: {reason}")]
    Launch { driver_name: String, reason: String },

    #[error("failed to connect drivers: {failures}")]
    ConnectFailures { failures: String },
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait ConnectRepository {
    fn list_names(&self) -> Result<Vec<String>>;
}

pub trait ServiceControl {
    fn ready(&self, driver_name: &str) -> bool;
}

pub trait ServiceLauncher {
    fn launch(&self, driver_name: &str, config_dir: &Path) -> std::result::Result<(), String>;
}

pub trait ConnectUseCase {
    fn connect_name(&self, driver_name: &str) -> Result<()>;
    fn connect_all(&self) -> Result<()>;
}

pub struct Application<'a, R, C, L> {
    config_dir: &'a Path,
    repository: &'a R,
    control: &'a C,
    launcher: &'a L,
}

impl<'a, R, C, L> Application<'a, R, C, L> {
    pub fn new(config_dir: &'a Path, repository: &'a R, control: &'a C, launcher: &'a L) -> Self {
        Self {
            config_dir,
            repository,
            control,
            launcher,
        }
    }
}

impl<R, C, L> Application<'_, R, C, L>
where
    R: ConnectRepository,
    C: ServiceControl,
    L: ServiceLauncher,
{
    fn connect_one(&self, driver_name: &str) -> std::result::Result<(), String> {
        if self.control.ready(driver_name) {
            return Ok(());
        }

        self.launcher.launch(driver_name, self.config_dir)
    }
}

impl<R, C, L> ConnectUseCase for Application<'_, R, C, L>
where
    R: ConnectRepository,
    C: ServiceControl,
    L: ServiceLauncher,
{
    fn connect_name(&self, driver_name: &str) -> Result<()> {
        self.connect_one(driver_name)
            .map_err(|reason| Error::Launch {
                driver_name: driver_name.to_owned(),
                reason,
            })
    }

    fn connect_all(&self) -> Result<()> {
        let mut failures = Vec::new();
        for name in self.repository.list_names()? {
            if let Err(reason) = self.connect_one(&name) {
                failures.push(format!("{name}: {reason}"));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::ConnectFailures {
                failures: failures.join(", "),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Application, ConnectRepository, ConnectUseCase, Result, ServiceControl, ServiceLauncher,
    };
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};

    #[derive(Default)]
    struct TestRepository {
        names: Vec<String>,
    }

    impl ConnectRepository for TestRepository {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(self.names.clone())
        }
    }

    #[derive(Default)]
    struct TestControl {
        ready_names: HashSet<String>,
    }

    impl ServiceControl for TestControl {
        fn ready(&self, driver_name: &str) -> bool {
            self.ready_names.contains(driver_name)
        }
    }

    #[derive(Default)]
    struct TestLauncher {
        failures: HashMap<String, String>,
    }

    impl ServiceLauncher for TestLauncher {
        fn launch(&self, driver_name: &str, _config_dir: &Path) -> std::result::Result<(), String> {
            match self.failures.get(driver_name) {
                Some(reason) => Err(reason.clone()),
                None => Ok(()),
            }
        }
    }

    struct TestConnectApp {
        config_dir: PathBuf,
        repository: TestRepository,
        control: TestControl,
        launcher: TestLauncher,
    }

    impl TestConnectApp {
        fn with_names<I, S>(mut self, names: I) -> Self
        where
            I: IntoIterator<Item = S>,
            S: AsRef<str>,
        {
            self.repository.names = names
                .into_iter()
                .map(|name| name.as_ref().to_owned())
                .collect();
            self
        }

        fn with_ready(mut self, driver_name: &str) -> Self {
            self.control.ready_names.insert(driver_name.to_owned());
            self
        }

        fn with_launch_failure(mut self, driver_name: &str, reason: &str) -> Self {
            self.launcher
                .failures
                .insert(driver_name.to_owned(), reason.to_owned());
            self
        }

        fn connect_all(&self) -> Result<()> {
            self.application().connect_all()
        }

        fn application(&self) -> Application<'_, TestRepository, TestControl, TestLauncher> {
            Application::new(
                &self.config_dir,
                &self.repository,
                &self.control,
                &self.launcher,
            )
        }
    }

    fn test_connect_app() -> TestConnectApp {
        TestConnectApp {
            config_dir: PathBuf::from("/tmp/anymount"),
            repository: TestRepository::default(),
            control: TestControl::default(),
            launcher: TestLauncher::default(),
        }
    }

    #[test]
    fn connect_all_collects_failures_without_stopping_successes() {
        let app = test_connect_app()
            .with_names(["alpha", "beta"])
            .with_ready("alpha")
            .with_launch_failure("beta", "spawn failed");

        let err = app.connect_all().expect_err("connect should fail");
        assert!(err.to_string().contains("beta"));
    }
}
