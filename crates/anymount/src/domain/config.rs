use std::{fmt, path::PathBuf, sync::Arc};

use super::{
    AuthorizationError, CredentialUpdate, DriverConfig, StorageConfig,
    StorageConnector,
};
use crate::drivers::oauth::RuntimeBounds;
use async_trait::async_trait;

pub struct Config {
    pub mount: MountConfig,
    pub driver: Option<Box<dyn DriverConfig>>,
}

pub struct MountConfig {
    pub name: String,
    pub path: PathBuf,
    pub storage: Box<dyn StorageConfig>,
}

/// An opaque identity for one loaded record within its repository.
///
/// The repository assigns an ID to a connector session. Use the same shared
/// ID for every host of that session, not a new ID for each connection.
#[derive(Debug, Default)]
pub struct ConfigRecordId {
    _private: (),
}

pub struct LoadedConfig {
    pub config: Config,
    pub connector: Arc<dyn StorageConnector>,
    pub record_id: Option<Arc<ConfigRecordId>>,
}

impl fmt::Debug for LoadedConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedConfig")
            .field("record_id", &self.record_id)
            .finish_non_exhaustive()
    }
}

impl LoadedConfig {
    /// Creates a connector without modifying the saved configuration.
    ///
    /// # Errors
    ///
    /// Propagates provider setup errors from connector creation.
    pub fn new(
        config: Config,
        identity: String,
    ) -> Result<Self, AuthorizationError> {
        let connector = config.mount.storage.create_connector(identity)?;
        Ok(Self {
            config,
            connector,
            record_id: None,
        })
    }
}

#[async_trait(?Send)]
pub trait ConfigRepository: RuntimeBounds {
    fn list(&self) -> Box<dyn Iterator<Item = LoadedConfig> + '_>;

    /// Finds a mount configuration by name in this repository.
    fn find(&self, name: &str) -> Result<LoadedConfig, ConfigNotFound> {
        self.list()
            .find(|record| record.config.mount.name == name)
            .ok_or_else(|| ConfigNotFound {
                name: name.to_owned(),
            })
    }

    /// Persists only the provider-designated fields on the selected record.
    ///
    /// Concurrent saves for the same loaded record must be serialized, even
    /// if an awaiting caller is canceled. An already committed generation
    /// supersedes requests for the same or an older generation.
    ///
    /// # Errors
    ///
    /// Returns an error if the record is unknown or the update fails to save.
    async fn save_credentials(
        &self,
        record_id: &ConfigRecordId,
        update: CredentialUpdate,
    ) -> Result<(), SaveCredentialsError>;
}

#[derive(Debug, thiserror::Error)]
#[error("configuration '{name}' was not found")]
pub struct ConfigNotFound {
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("could not save the latest credential configuration")]
pub struct SaveCredentialsError;
