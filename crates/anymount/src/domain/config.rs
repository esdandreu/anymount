use std::{fmt, path::PathBuf, sync::Arc};

use super::{
    AuthorizationError, ConfigurationPersistence, DriverConfig, StorageConfig,
    StorageSession,
};

pub struct Config {
    pub mount: MountConfig,
    pub driver: Option<Box<dyn DriverConfig>>,
}

pub struct MountConfig {
    pub name: String,
    pub path: PathBuf,
    pub storage: Box<dyn StorageConfig>,
}

/// Retains a saved configuration and its separate, live storage session.
pub struct LoadedConfig {
    pub config: Config,
    pub session: Arc<dyn StorageSession>,
    pub persistence: Option<Arc<dyn ConfigurationPersistence>>,
}

impl fmt::Debug for LoadedConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedConfig")
            .field("has_persistence", &self.persistence.is_some())
            .finish_non_exhaustive()
    }
}

impl LoadedConfig {
    /// Opens an independent session for this configuration record.
    ///
    /// Reuse this record or clone its session handle to share live state.
    /// The saved configuration is unchanged by authorization and renewal.
    ///
    /// # Errors
    ///
    /// Returns a provider setup error if the configuration cannot be used.
    pub fn new(
        config: Config,
        identity: String,
    ) -> Result<Self, AuthorizationError> {
        let session = config.mount.storage.open_session(identity)?;
        Ok(Self {
            config,
            session,
            persistence: None,
        })
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
