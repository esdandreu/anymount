use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;

use crate::domain::{
    ConnectStorageError, HostSaveError, SaveStatus, Storage,
    driver::DriverConfig, storage::StorageConfig,
};

pub struct Config {
    pub mount: MountConfig,
    pub driver: Option<Box<dyn DriverConfig>>,
}

pub struct MountConfig {
    pub name: String,
    pub path: PathBuf,
    pub storage: Box<dyn StorageConfig>,
    /// Explicit runtime ownership shared by connections to this record.
    pub session: Option<Arc<dyn MountSession>>,
}

/// Connects a host-owned configuration and independently persists its updates.
///
/// Hosts retain one session per configuration record. Storage requests never
/// call persistence; native hosts drive update saving in a separate task.
#[async_trait(?Send)]
pub trait MountSession: Send + Sync {
    /// Returns the retained provider authorization boundary for host interaction.
    fn authorization(&self) -> &dyn crate::domain::Authorization;

    /// Connects using the record's shared live credentials.
    ///
    /// # Errors
    ///
    /// Returns an error when this configuration cannot be connected.
    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError>;

    /// Reports persistence independently of storage access.
    fn save_status(&self) -> SaveStatus;

    /// Retries saving the newest credentials without repeating authorization.
    ///
    /// # Errors
    ///
    /// Returns a host save error while retaining the pending credentials.
    async fn save_pending(&self) -> Result<(), HostSaveError>;

    /// Waits for a newer credential generation without polling.
    async fn wait_for_update(&self, after_generation: u64) -> u64;
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
