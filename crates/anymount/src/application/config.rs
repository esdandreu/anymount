use crate::domain::driver::{DriverConfig, StorageConfig};
use std::num::ParseIntError;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] crate::config::Error),

    #[error("driver '{name}' already exists")]
    DuplicateDriver { name: String },

    #[error("'{key}' only applies to onedrive storage")]
    InvalidStorageKey { key: String },

    #[error(
        "unknown key '{key}'. Valid keys: path, storage.root, storage.endpoint, \
         storage.access_token, storage.refresh_token, storage.client_id, \
         storage.token_expiry_buffer_secs"
    )]
    UnknownKey { key: String },

    #[error("invalid integer value {value}: {source}")]
    ParseInteger {
        value: String,
        #[source]
        source: ParseIntError,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait ConfigRepository {
    fn list_names(&self) -> Result<Vec<String>>;
    fn read_spec(&self, name: &str) -> Result<DriverConfig>;
    fn write_spec(&self, spec: &DriverConfig) -> Result<()>;
    fn remove(&self, name: &str) -> Result<()>;
}

pub trait ConfigUseCase {
    fn list(&self) -> Result<Vec<String>>;
    fn read(&self, name: &str) -> Result<DriverConfig>;
    fn add(&self, spec: DriverConfig) -> Result<()>;
    fn remove(&self, name: &str) -> Result<()>;
    fn set(&self, name: &str, key: &str, value: &str) -> Result<()>;
}

pub struct Application<'a, R> {
    repository: &'a R,
}

impl<'a, R> Application<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }
}

impl<R> ConfigUseCase for Application<'_, R>
where
    R: ConfigRepository,
{
    fn list(&self) -> Result<Vec<String>> {
        self.repository.list_names()
    }

    fn read(&self, name: &str) -> Result<DriverConfig> {
        self.repository.read_spec(name)
    }

    fn add(&self, spec: DriverConfig) -> Result<()> {
        if self.repository.list_names()?.contains(&spec.name) {
            return Err(Error::DuplicateDriver {
                name: spec.name.clone(),
            });
        }
        self.repository.write_spec(&spec)
    }

    fn remove(&self, name: &str) -> Result<()> {
        self.repository.remove(name)
    }

    fn set(&self, name: &str, key: &str, value: &str) -> Result<()> {
        let mut spec = self.repository.read_spec(name)?;
        apply_set(&mut spec, key, value)?;
        self.repository.write_spec(&spec)
    }
}

pub(crate) fn apply_set(spec: &mut DriverConfig, key: &str, value: &str) -> Result<()> {
    match key {
        "path" => {
            spec.path = PathBuf::from(value);
        }
        "storage.root" => match &mut spec.storage {
            StorageConfig::Local { root } | StorageConfig::OneDrive { root, .. } => {
                *root = PathBuf::from(value);
            }
        },
        "storage.endpoint" => match &mut spec.storage {
            StorageConfig::OneDrive { endpoint, .. } => {
                *endpoint = value.to_owned();
            }
            StorageConfig::Local { .. } => {
                return Err(Error::InvalidStorageKey {
                    key: key.to_owned(),
                });
            }
        },
        "storage.access_token" => match &mut spec.storage {
            StorageConfig::OneDrive { access_token, .. } => {
                *access_token = Some(value.to_owned());
            }
            StorageConfig::Local { .. } => {
                return Err(Error::InvalidStorageKey {
                    key: key.to_owned(),
                });
            }
        },
        "storage.refresh_token" => match &mut spec.storage {
            StorageConfig::OneDrive { refresh_token, .. } => {
                *refresh_token = Some(value.to_owned());
            }
            StorageConfig::Local { .. } => {
                return Err(Error::InvalidStorageKey {
                    key: key.to_owned(),
                });
            }
        },
        "storage.client_id" => match &mut spec.storage {
            StorageConfig::OneDrive { client_id, .. } => {
                *client_id = Some(value.to_owned());
            }
            StorageConfig::Local { .. } => {
                return Err(Error::InvalidStorageKey {
                    key: key.to_owned(),
                });
            }
        },
        "storage.token_expiry_buffer_secs" => match &mut spec.storage {
            StorageConfig::OneDrive {
                token_expiry_buffer_secs,
                ..
            } => {
                let secs = parse_u64(value.to_owned())?;
                *token_expiry_buffer_secs = Some(secs);
            }
            StorageConfig::Local { .. } => {
                return Err(Error::InvalidStorageKey {
                    key: key.to_owned(),
                });
            }
        },
        _ => {
            return Err(Error::UnknownKey {
                key: key.to_owned(),
            });
        }
    }
    Ok(())
}

fn parse_u64(value: String) -> Result<u64> {
    value
        .parse::<u64>()
        .map_err(|source| Error::ParseInteger { value, source })
}

#[cfg(test)]
mod tests {
    use super::{Application, ConfigRepository, ConfigUseCase, Error, Result};
    use crate::domain::driver::{DriverConfig, StorageConfig, TelemetrySpec};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[derive(Default)]
    struct TestRepository {
        specs: RefCell<HashMap<String, DriverConfig>>,
    }

    impl ConfigRepository for TestRepository {
        fn list_names(&self) -> Result<Vec<String>> {
            let mut names = self.specs.borrow().keys().cloned().collect::<Vec<_>>();
            names.sort();
            Ok(names)
        }

        fn read_spec(&self, name: &str) -> Result<DriverConfig> {
            self.specs
                .borrow()
                .get(name)
                .cloned()
                .ok_or_else(|| Error::DuplicateDriver {
                    name: format!("missing:{name}"),
                })
        }

        fn write_spec(&self, spec: &DriverConfig) -> Result<()> {
            self.specs
                .borrow_mut()
                .insert(spec.name.clone(), spec.clone());
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<()> {
            self.specs.borrow_mut().remove(name);
            Ok(())
        }
    }

    struct TestConfigApp {
        repository: TestRepository,
    }

    impl TestConfigApp {
        fn with_existing(self, spec: DriverConfig) -> Self {
            self.repository
                .specs
                .borrow_mut()
                .insert(spec.name.clone(), spec);
            self
        }

        fn add(&self, spec: DriverConfig) -> Result<()> {
            self.application().add(spec)
        }

        fn set(&self, name: &str, key: &str, value: &str) -> Result<()> {
            self.application().set(name, key, value)
        }

        fn read(&self, name: &str) -> Result<DriverConfig> {
            self.application().read(name)
        }

        fn application(&self) -> Application<'_, TestRepository> {
            Application::new(&self.repository)
        }
    }

    fn test_config_app() -> TestConfigApp {
        TestConfigApp {
            repository: TestRepository::default(),
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

    fn onedrive_driver_spec(name: &str) -> DriverConfig {
        DriverConfig {
            name: name.to_owned(),
            path: PathBuf::from(format!("/mnt/{name}")),
            storage: StorageConfig::OneDrive {
                root: PathBuf::from("/"),
                endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
                access_token: None,
                refresh_token: Some("refresh".to_owned()),
                client_id: Some("client".to_owned()),
                token_expiry_buffer_secs: Some(60),
            },
            telemetry: TelemetrySpec::default(),
        }
    }

    #[test]
    fn add_rejects_duplicate_driver_names() {
        let app = test_config_app().with_existing(local_driver_spec("alpha"));
        let err = app
            .add(local_driver_spec("alpha"))
            .expect_err("add should fail");
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn set_updates_storage_endpoint() {
        let app = test_config_app().with_existing(onedrive_driver_spec("alpha"));
        app.set("alpha", "storage.endpoint", "https://example.test/v1")
            .expect("set should work");

        let spec = app.read("alpha").expect("read should work");
        assert_eq!(
            spec.onedrive_endpoint().as_deref(),
            Some("https://example.test/v1")
        );
    }
}
