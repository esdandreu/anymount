use std::{fmt, path::PathBuf, sync::Arc};

use super::{DriverConfig, StorageConfig, StorageConnector};

pub struct Config {
    pub mount: MountConfig,
    pub driver: Option<Box<dyn DriverConfig>>,
}

pub struct MountConfig {
    pub name: String,
    pub path: PathBuf,
    pub storage: Box<dyn StorageConfig>,
}

/// A saved configuration paired with its live storage connector.
pub struct LoadedConfig {
    pub config: Config,
    pub connector: Arc<dyn StorageConnector>,
}

impl fmt::Debug for LoadedConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedConfig").finish_non_exhaustive()
    }
}

impl LoadedConfig {
    /// Creates a connector without modifying the saved configuration.
    pub fn new(config: Config, identity: String) -> Self {
        let connector = config.mount.storage.create_connector(identity);
        Self { config, connector }
    }
}

pub trait ConfigRepository {
    type Iter<'a>: Iterator<Item = LoadedConfig>
    where
        Self: 'a;

    fn list(&self) -> Self::Iter<'_>;

    /// Finds a mount configuration by name in this repository.
    fn find(&self, name: &str) -> Result<LoadedConfig, ConfigNotFound> {
        self.list()
            .find(|record| record.config.mount.name == name)
            .ok_or_else(|| ConfigNotFound {
                name: name.to_owned(),
            })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("configuration '{name}' was not found")]
pub struct ConfigNotFound {
    pub name: String,
}
