use std::path::PathBuf;

use super::{Mount, Storage};

#[typetag::serde(tag = "type")]
pub trait DriverConfig: Send + Sync {
    fn init(&self) -> Box<dyn Driver>;

    fn kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

pub trait Driver: Send + Sync {
    /// Registers a mount. If a mount was already registered, with the same
    /// name and path the storage will be re-configured.
    fn mount(
        &self,
        name: String,
        path: PathBuf,
        storage: Box<dyn Storage>,
    ) -> Result<Box<dyn Mount>, MountError>;

    fn kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Lists registered mounts.
    fn list_mounts(
        &self,
    ) -> Result<Box<dyn Iterator<Item = Box<dyn Mount>>>, ListMountsError>;
}

#[derive(Debug, thiserror::Error)]
pub enum MountError {
    /// A path conflict. Typically the path is already mounted or not accessible
    /// in some way.
    #[error("failed to mount at {path}: {message}")]
    CannotMountAtPath { path: PathBuf, message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ListMountsError {
    #[error("failed to list mounts: {message}")]
    CannotListMounts { message: String },
}

/// Driver config with a priority value to determine which is the default driver
/// configuration. The driver configuration with the highest priority will be
/// used by default when the mount does not specify any driver. Users of this
/// library can override the default driver by submitting a their own drivers
/// with a higher priority.
pub struct DefaultDriverConfig {
    pub priority: i32,
    pub config: &'static dyn DriverConfig,
}

inventory::collect!(DefaultDriverConfig);
