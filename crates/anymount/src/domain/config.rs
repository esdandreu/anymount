use std::path::PathBuf;

use crate::domain::{driver::DriverConfig, storage::StorageConfig};

pub struct Config {
    pub mount: MountConfig,
    pub driver: Option<Box<dyn DriverConfig>>,
}

pub struct MountConfig {
    pub name: String,
    pub path: PathBuf,
    pub storage: Box<dyn StorageConfig>,
}

pub trait ConfigRepository {
    type Iter<'a>: Iterator<Item = Config>
    where
        Self: 'a;

    fn list(&self) -> Self::Iter<'_>;

    /// Finds a mount configuration by name in this repository.
    fn find(&self, name: &str) -> Result<Config, ConfigNotFound> {
        self.list()
            .find(|config| config.mount.name == name)
            .map(|config| config)
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
