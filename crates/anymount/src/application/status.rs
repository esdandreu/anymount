use crate::application::types::DriverStatusRow;
use crate::domain::driver::{DriverConfig, StorageConfig};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] crate::config::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusEntry {
    Loaded(DriverConfig),
    Error { name: String, detail: String },
}

pub trait StatusRepository {
    fn list_entries(&self) -> Result<Vec<StatusEntry>>;
}

pub trait ServiceControl {
    fn ready(&self, driver_name: &str) -> bool;
}

pub trait StatusUseCase {
    fn list(&self) -> Result<Vec<DriverStatusRow>>;
}

pub struct Application<'a, R, C> {
    repository: &'a R,
    control: &'a C,
}

impl<'a, R, C> Application<'a, R, C> {
    pub fn new(repository: &'a R, control: &'a C) -> Self {
        Self {
            repository,
            control,
        }
    }
}

impl<R, C> StatusUseCase for Application<'_, R, C>
where
    R: StatusRepository,
    C: ServiceControl,
{
    fn list(&self) -> Result<Vec<DriverStatusRow>> {
        self.repository
            .list_entries()?
            .into_iter()
            .map(|entry| match entry {
                StatusEntry::Loaded(spec) => Ok(DriverStatusRow {
                    ready: self.control.ready(&spec.name),
                    storage: Some(storage_label(&spec.storage).to_owned()),
                    path: Some(spec.path),
                    name: spec.name,
                    error: None,
                }),
                StatusEntry::Error { name, detail } => Ok(DriverStatusRow {
                    name,
                    storage: None,
                    path: None,
                    ready: false,
                    error: Some(detail),
                }),
            })
            .collect()
    }
}

fn storage_label(storage: &StorageConfig) -> &'static str {
    match storage {
        StorageConfig::Local { .. } => "local",
        StorageConfig::OneDrive { .. } => "onedrive",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Application, Result, ServiceControl, StatusEntry, StatusRepository, StatusUseCase,
    };
    use crate::domain::driver::{DriverConfig, StorageConfig, TelemetrySpec};
    use std::collections::HashSet;
    use std::path::PathBuf;

    #[derive(Default)]
    struct TestRepository {
        entries: Vec<StatusEntry>,
    }

    impl StatusRepository for TestRepository {
        fn list_entries(&self) -> Result<Vec<StatusEntry>> {
            Ok(self.entries.clone())
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

    struct TestStatusApp {
        repository: TestRepository,
        control: TestControl,
    }

    impl TestStatusApp {
        fn with_spec(mut self, spec: DriverConfig) -> Self {
            self.repository.entries.push(StatusEntry::Loaded(spec));
            self
        }

        fn list(&self) -> Result<Vec<crate::application::types::DriverStatusRow>> {
            self.application().list()
        }

        fn application(&self) -> Application<'_, TestRepository, TestControl> {
            Application::new(&self.repository, &self.control)
        }
    }

    fn test_status_app() -> TestStatusApp {
        TestStatusApp {
            repository: TestRepository::default(),
            control: TestControl::default(),
        }
    }

    fn local_driver_spec(name: &str) -> DriverConfig {
        DriverConfig {
            name: name.to_owned(),
            path: PathBuf::from(format!("/mnt/{name}")),
            storage: StorageConfig::Local {
                root: PathBuf::from(format!("/data/{name}")),
            },
            telemetry: TelemetrySpec::default(),
        }
    }

    #[test]
    fn status_includes_not_running_entries() {
        let app = test_status_app().with_spec(local_driver_spec("demo"));

        let rows = app.list().expect("status should work");
        assert_eq!(rows[0].name, "demo");
        assert!(!rows[0].ready);
    }
}
