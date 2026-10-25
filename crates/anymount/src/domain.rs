//! Defines platform-neutral driver, mount, and storage contracts.

mod config;
mod driver;
mod mount;
mod platform_send_sync;
mod started_authorization;
mod storage;

pub use config::{Config, ConfigNotFound, ConfigRepository, MountConfig};
#[cfg(not(target_arch = "wasm32"))]
pub use driver::DefaultDriverConfig;
pub use driver::{Driver, DriverConfig, MountError};
pub use mount::{
    ConnectMountError, DisconnectMountError, Mount, UnregisterMountError,
};
pub use platform_send_sync::PlatformSendSync;
pub use started_authorization::{AuthStorageError, StartedAuthorization};
pub use storage::{
    ConnectStorageError, DirEntry, InvalidStoragePath, ReadDirError,
    ReadFileAtError, Storage, StorageConfig, StoragePath, WriteAt,
    WriteAtError,
};

#[cfg(test)]
pub use storage::MockStorage;
