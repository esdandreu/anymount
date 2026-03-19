use crate::application::types::ProvideRequest;
use crate::domain::provider::ProviderSpec;
use crate::telemetry::OtelHandles;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] crate::config::Error),

    #[error(transparent)]
    Telemetry(#[from] crate::telemetry::OtlpInitError),

    #[error("failed to load provider spec {provider_name}: {reason}")]
    Repository {
        provider_name: String,
        reason: String,
    },

    #[error("failed to host provider {provider_name}: {reason}")]
    Host {
        provider_name: String,
        reason: String,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait ProvideRepository {
    fn read_spec(&self, name: &str) -> Result<ProviderSpec>;
}

pub trait TelemetryFactory {
    fn build(&self, spec: &ProviderSpec) -> Result<Option<OtelHandles>>;
}

pub trait ProviderRuntimeHost {
    fn run(&self, request: ProvideRequest, telemetry: Option<OtelHandles>) -> Result<()>;
}

pub trait ProvideUseCase {
    fn run_named(&self, name: &str) -> Result<()>;
    fn run_inline(&self, spec: ProviderSpec) -> Result<()>;
}

pub struct Application<'a, R, T, H> {
    repository: &'a R,
    telemetry: &'a T,
    host: &'a H,
}

impl<'a, R, T, H> Application<'a, R, T, H> {
    pub fn new(repository: &'a R, telemetry: &'a T, host: &'a H) -> Self {
        Self {
            repository,
            telemetry,
            host,
        }
    }
}

impl<R, T, H> ProvideUseCase for Application<'_, R, T, H>
where
    R: ProvideRepository,
    T: TelemetryFactory,
    H: ProviderRuntimeHost,
{
    fn run_named(&self, name: &str) -> Result<()> {
        let spec = self.repository.read_spec(name)?;
        self.run_request(ProvideRequest {
            spec,
            control_name: Some(name.to_owned()),
        })
    }

    fn run_inline(&self, spec: ProviderSpec) -> Result<()> {
        self.run_request(ProvideRequest {
            spec,
            control_name: None,
        })
    }
}

impl<R, T, H> Application<'_, R, T, H>
where
    T: TelemetryFactory,
    H: ProviderRuntimeHost,
{
    fn run_request(&self, request: ProvideRequest) -> Result<()> {
        let telemetry = self.telemetry.build(&request.spec)?;
        self.host.run(request, telemetry)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Application, Error, ProvideRepository, ProvideUseCase, ProviderRuntimeHost, Result,
        TelemetryFactory,
    };
    use crate::application::types::ProvideRequest;
    use crate::domain::provider::{ProviderSpec, StorageSpec, TelemetrySpec};
    use crate::telemetry::OtelHandles;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[derive(Default)]
    struct TestRepository {
        specs: HashMap<String, ProviderSpec>,
        reads: Cell<usize>,
    }

    impl ProvideRepository for TestRepository {
        fn read_spec(&self, name: &str) -> Result<ProviderSpec> {
            self.reads.set(self.reads.get() + 1);
            self.specs
                .get(name)
                .cloned()
                .ok_or_else(|| Error::Repository {
                    provider_name: name.to_owned(),
                    reason: "missing spec".to_owned(),
                })
        }
    }

    #[derive(Default)]
    struct TestTelemetryFactory;

    impl TelemetryFactory for TestTelemetryFactory {
        fn build(&self, _spec: &ProviderSpec) -> Result<Option<OtelHandles>> {
            Ok(None)
        }
    }

    #[derive(Default)]
    struct TestHost {
        hosted: RefCell<Vec<String>>,
    }

    impl ProviderRuntimeHost for TestHost {
        fn run(&self, request: ProvideRequest, _telemetry: Option<OtelHandles>) -> Result<()> {
            self.hosted.borrow_mut().push(request.spec.name);
            Ok(())
        }
    }

    struct TestProvideApp {
        repository: TestRepository,
        telemetry: TestTelemetryFactory,
        host: TestHost,
    }

    impl TestProvideApp {
        fn with_spec(mut self, spec: ProviderSpec) -> Self {
            self.repository.specs.insert(spec.name.clone(), spec);
            self
        }

        fn run_named(&self, name: &str) -> Result<()> {
            self.application().run_named(name)
        }

        fn run_inline(&self, spec: ProviderSpec) -> Result<()> {
            self.application().run_inline(spec)
        }

        fn hosted_specs(&self) -> Vec<String> {
            self.host.hosted.borrow().clone()
        }

        fn repository_reads(&self) -> usize {
            self.repository.reads.get()
        }

        fn application(&self) -> Application<'_, TestRepository, TestTelemetryFactory, TestHost> {
            Application::new(&self.repository, &self.telemetry, &self.host)
        }
    }

    fn test_provide_app() -> TestProvideApp {
        TestProvideApp {
            repository: TestRepository::default(),
            telemetry: TestTelemetryFactory,
            host: TestHost::default(),
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
    fn named_provide_loads_spec_and_starts_host() {
        let app = test_provide_app().with_spec(local_provider_spec("demo"));
        app.run_named("demo").expect("provide should work");
        assert_eq!(app.hosted_specs(), vec!["demo".to_owned()]);
    }

    #[test]
    fn inline_provide_skips_repository_lookup() {
        let app = test_provide_app();
        app.run_inline(local_provider_spec("inline"))
            .expect("inline provide should work");
        assert_eq!(app.repository_reads(), 0);
    }
}
