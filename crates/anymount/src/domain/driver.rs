use std::path::PathBuf;

use super::{Mount, MountConfig};

#[typetag::serde(tag = "type")]
pub trait DriverConfig: Send + Sync {
    fn init(&self) -> Box<dyn Driver>;

    fn kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

pub trait Driver: Send + Sync {
    /// Registers a mount. No-op if a mount was already registered with the same
    /// configuration.
    fn mount(&self, config: &MountConfig)
    -> Result<Box<dyn Mount>, MountError>;

    /// Unregisters a mount. No-op if the mount is not registered.
    fn unmount(&self, name: &str) -> Result<(), MountError>;

    fn kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MountError {
    /// A path conflict. Typically the path is already mounted or not accessible
    /// in some way.
    #[error("failed to mount at {path}: {message}")]
    CannotMountAtPath { path: PathBuf, message: String },
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
