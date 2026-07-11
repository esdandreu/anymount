use std::sync::OnceLock;

use super::ConfigDir;
use crate::domain::{Config, ConfigRepository, Driver, Mount};

pub struct Anymount<C = ConfigDir>
where
    C: ConfigRepository,
{
    config_repository: C,
}

impl<C> Anymount<C>
where
    C: ConfigRepository,
{
    pub fn new(config_repository: C) -> Self {
        Self { config_repository }
    }

    pub fn connect(&self, name: &str) -> Result<Box<dyn Mount>, ConnectError> {
        let config = self
            .config_repository
            .list()
            .find(|config| config.name == name)
            .ok_or_else(|| ConnectError::ConfigNotFound {
                name: name.to_owned(),
            })?;

        Ok(mount(config)?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("mount configuration '{name}' was not found")]
    ConfigNotFound { name: String },

    #[error("failed to mount: {0}")]
    CannotMount(#[from] MountError),
}

impl Default for Anymount<ConfigDir> {
    fn default() -> Self {
        Self::new(ConfigDir::default())
    }
}

fn mount(config: Config) -> Result<Box<dyn Mount>, MountError> {
    let storage = config.storage.connect()?;

    let driver = match config.driver {
        Some(driver) => driver.init(),
        None => get_default_driver().ok_or(MountError::NoDefaultDriver)?,
    };

    Ok(driver.mount(config.name, config.path, storage)?)
}

#[derive(Debug, thiserror::Error)]
pub enum MountError {
    #[error("failed to connect storage: {0}")]
    CannotConnectStorage(#[from] super::domain::storage::ConnectStorageError),

    #[error("no default driver is available")]
    NoDefaultDriver,

    #[error("failed to mount: {0}")]
    CannotMount(#[from] super::domain::driver::MountError),
}

/// Memoized default driver for get_default_driver.
static DEFAULT_DRIVER: OnceLock<Option<&'static dyn Driver>> = OnceLock::new();

/// Returns the default driver if there is any.
pub fn get_default_driver() -> Option<&'static dyn Driver> {
    *DEFAULT_DRIVER.get_or_init(|| {
        inventory::iter::<super::domain::driver::DefaultDriver>
            .into_iter()
            .max_by_key(|entry| entry.priority)
            .map(|entry| entry.driver)
    })
}

// TODO: Tests with mocked ConfigRepository and Driver
