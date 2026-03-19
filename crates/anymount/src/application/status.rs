use crate::application::types::ProviderStatusRow;
use crate::domain::provider::{ProviderSpec, StorageSpec};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] crate::config::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusEntry {
    Loaded(ProviderSpec),
    Error { name: String, detail: String },
}

pub trait StatusRepository {
    fn list_entries(&self) -> Result<Vec<StatusEntry>>;
}

pub trait ServiceControl {
    fn ready(&self, provider_name: &str) -> bool;
}

pub trait StatusUseCase {
    fn list(&self) -> Result<Vec<ProviderStatusRow>>;
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
    fn list(&self) -> Result<Vec<ProviderStatusRow>> {
        self.repository
            .list_entries()?
            .into_iter()
            .map(|entry| match entry {
                StatusEntry::Loaded(spec) => Ok(ProviderStatusRow {
                    ready: self.control.ready(&spec.name),
                    storage: Some(storage_label(&spec.storage).to_owned()),
                    path: Some(spec.path),
                    name: spec.name,
                    error: None,
                }),
                StatusEntry::Error { name, detail } => Ok(ProviderStatusRow {
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

fn storage_label(storage: &StorageSpec) -> &'static str {
    match storage {
        StorageSpec::Local { .. } => "local",
        StorageSpec::OneDrive { .. } => "onedrive",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Application, Result, ServiceControl, StatusEntry, StatusRepository, StatusUseCase,
    };
    use crate::domain::provider::{ProviderSpec, StorageSpec, TelemetrySpec};
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
        fn ready(&self, provider_name: &str) -> bool {
            self.ready_names.contains(provider_name)
        }
    }

    struct TestStatusApp {
        repository: TestRepository,
        control: TestControl,
    }

    impl TestStatusApp {
        fn with_spec(mut self, spec: ProviderSpec) -> Self {
            self.repository.entries.push(StatusEntry::Loaded(spec));
            self
        }

        fn list(&self) -> Result<Vec<crate::application::types::ProviderStatusRow>> {
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

    fn local_provider_spec(name: &str) -> ProviderSpec {
        ProviderSpec {
            name: name.to_owned(),
            path: PathBuf::from(format!("/mnt/{name}")),
            storage: StorageSpec::Local {
                root: PathBuf::from(format!("/data/{name}")),
            },
            telemetry: TelemetrySpec::default(),
        }
    }

    #[test]
    fn status_includes_not_running_entries() {
        let app = test_status_app().with_spec(local_provider_spec("demo"));

        let rows = app.list().expect("status should work");
        assert_eq!(rows[0].name, "demo");
        assert!(!rows[0].ready);
    }
}
