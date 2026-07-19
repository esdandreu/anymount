use crate::ConfigRepository;
use crate::domain::{Config, Mount};
use crate::drivers::init_driver_or_default;

pub fn mount(config: Config) -> Result<Box<dyn Mount>, MountError> {
    let storage = config.storage.connect()?;

    let driver = init_driver_or_default(config.driver)?;

    Ok(driver.mount(config.name, config.path, storage)?)
}

#[derive(Debug, thiserror::Error)]
pub enum MountError {
    #[error("failed to connect storage: {0}")]
    CannotConnectStorage(#[from] crate::domain::storage::ConnectStorageError),

    #[error(transparent)]
    NoDefaultDriver(#[from] crate::drivers::NoDefaultDriver),

    #[error("failed to mount: {0}")]
    CannotMount(#[from] crate::domain::driver::MountError),
}

pub fn find_and_mount(
    config_repository: &impl ConfigRepository,
    name: &str,
) -> Result<Box<dyn Mount>, FindAndMountError> {
    let config = config_repository
        .list()
        .find(|config| config.name == name)
        .ok_or_else(|| FindAndMountError::ConfigNotFound {
            name: name.to_owned(),
        })?;

    Ok(mount(config)?)
}

#[derive(Debug, thiserror::Error)]
pub enum FindAndMountError {
    #[error("mount configuration '{name}' was not found")]
    ConfigNotFound { name: String },

    #[error("failed to mount: {0}")]
    CannotMount(#[from] MountError),
}

// TODO: Tests with mocked ConfigRepository and Driver
