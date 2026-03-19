#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] crate::config::Error),

    #[error("failed to disconnect provider {provider_name}: {reason}")]
    Disconnect {
        provider_name: String,
        reason: String,
    },

    #[error("failed to disconnect providers: {failures}")]
    DisconnectFailures { failures: String },
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait DisconnectRepository {
    fn list_names(&self) -> Result<Vec<String>>;
}

pub trait ServiceControl {
    fn disconnect(&self, provider_name: &str) -> std::result::Result<(), String>;
}

pub trait DisconnectUseCase {
    fn disconnect_name(&self, provider_name: &str) -> Result<()>;
    fn disconnect_all(&self) -> Result<()>;
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

impl<R, C> DisconnectUseCase for Application<'_, R, C>
where
    R: DisconnectRepository,
    C: ServiceControl,
{
    fn disconnect_name(&self, provider_name: &str) -> Result<()> {
        self.control
            .disconnect(provider_name)
            .map_err(|reason| Error::Disconnect {
                provider_name: provider_name.to_owned(),
                reason,
            })
    }

    fn disconnect_all(&self) -> Result<()> {
        let mut failures = Vec::new();
        for name in self.repository.list_names()? {
            if let Err(reason) = self.control.disconnect(&name) {
                failures.push(format!("{name}: {reason}"));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::DisconnectFailures {
                failures: failures.join(", "),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Application, DisconnectRepository, DisconnectUseCase, Result, ServiceControl};
    use std::collections::HashMap;

    #[derive(Default)]
    struct TestRepository {
        names: Vec<String>,
    }

    impl DisconnectRepository for TestRepository {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(self.names.clone())
        }
    }

    #[derive(Default)]
    struct TestControl {
        failures: HashMap<String, String>,
    }

    impl ServiceControl for TestControl {
        fn disconnect(&self, provider_name: &str) -> std::result::Result<(), String> {
            match self.failures.get(provider_name) {
                Some(reason) => Err(reason.clone()),
                None => Ok(()),
            }
        }
    }

    struct TestDisconnectApp {
        repository: TestRepository,
        control: TestControl,
    }

    impl TestDisconnectApp {
        fn disconnect_name(&self, provider_name: &str) -> Result<()> {
            self.application().disconnect_name(provider_name)
        }

        fn application(&self) -> Application<'_, TestRepository, TestControl> {
            Application::new(&self.repository, &self.control)
        }
    }

    fn test_disconnect_app() -> TestDisconnectApp {
        TestDisconnectApp {
            repository: TestRepository::default(),
            control: TestControl::default(),
        }
    }

    #[test]
    fn disconnect_name_is_idempotent_when_service_is_missing() {
        let app = test_disconnect_app();
        app.disconnect_name("demo")
            .expect("missing service should be fine");
    }
}
