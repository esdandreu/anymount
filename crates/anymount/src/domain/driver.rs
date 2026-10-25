use std::path::PathBuf;

use async_trait::async_trait;

use super::storage::ConnectStorageError;
use super::{Mount, MountConfig};

#[typetag::serde(tag = "type")]
pub trait DriverConfig: super::PlatformSendSync {
    fn init(&self) -> Box<dyn Driver>;

    fn kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

#[cfg_attr(test, mockall::automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait Driver: super::PlatformSendSync {
    /// Registers a mount. No-op if a mount was already registered with the same
    /// configuration.
    async fn mount(
        &self,
        config: &MountConfig,
    ) -> Result<Box<dyn Mount>, MountError>;

    /// Unregisters a mount. No-op if the mount is not registered.
    async fn unmount(&self, name: &str) -> Result<(), MountError>;

    fn kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MountError {
    /// The storage configuration could not be connected.
    #[error(transparent)]
    CannotConnectStorage(#[from] ConnectStorageError),

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
#[cfg(not(target_arch = "wasm32"))]
pub struct DefaultDriverConfig {
    pub priority: i32,
    pub config: &'static dyn DriverConfig,
}

#[cfg(not(target_arch = "wasm32"))]
inventory::collect!(DefaultDriverConfig);

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::test_utils::mocks::{EmptyStorageConfig, NoOpMount};

    #[tokio::test]
    async fn driver_lifecycle_is_callable_through_trait_object() {
        let mut mock_driver = MockDriver::new();
        mock_driver
            .expect_mount()
            .withf(|config| config.name == "test")
            .times(1)
            .returning(|_| {
                let mount: Box<dyn Mount> = Box::new(NoOpMount);
                Ok(mount)
            });
        mock_driver
            .expect_unmount()
            .withf(|name| name == "no-op")
            .times(1)
            .returning(|_| Ok(()));
        let driver: Box<dyn Driver> = Box::new(mock_driver);
        let config = MountConfig {
            name: "test".to_owned(),
            path: PathBuf::from("/tmp/test"),
            storage: Box::new(EmptyStorageConfig),
        };
        let mount = driver.mount(&config).await.expect("mount should succeed");
        mount.connect().await.expect("mount should connect");
        mount.disconnect().await.expect("mount should disconnect");
        mount.unregister().await.expect("mount should unregister");
        driver
            .unmount(mount.name())
            .await
            .expect("unmount should succeed");
    }
}
