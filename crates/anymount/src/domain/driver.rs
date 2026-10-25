use std::path::PathBuf;

use async_trait::async_trait;

use super::{Mount, MountConfig};

#[typetag::serde(tag = "type")]
pub trait DriverConfig: Send + Sync {
    fn init(&self) -> Box<dyn Driver>;

    fn kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

#[async_trait]
pub trait Driver: Send + Sync {
    /// Registers a mount. No-op if a mount was already registered with the same
    /// configuration.
    async fn mount(
        &self,
        config: &MountConfig,
    ) -> Result<Box<dyn Mount>, MountError>;

    /// Unregisters a mount. No-op if the mount is not registered.
    async fn unmount(&self, _name: &str) -> Result<(), MountError> {
        Ok(())
    }

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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::domain::{
        ConnectMountError, ConnectStorageError, DisconnectMountError, Mount,
        MountConfig, Storage, StorageConfig,
    };

    #[derive(serde::Serialize, serde::Deserialize)]
    struct EmptyStorageConfig;

    #[typetag::serde(name = "empty")]
    impl StorageConfig for EmptyStorageConfig {
        fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
            Ok(Box::new(EmptyStorage))
        }
    }

    struct EmptyStorage;

    impl Storage for EmptyStorage {}

    struct TestMount;

    impl Mount for TestMount {
        fn name(&self) -> &str {
            "test"
        }

        fn root(&self) -> &Path {
            Path::new("/tmp/test")
        }

        fn is_connected(&self) -> bool {
            true
        }

        fn connect(&self) -> Result<(), ConnectMountError> {
            Ok(())
        }

        fn disconnect(&self) -> Result<(), DisconnectMountError> {
            Ok(())
        }
    }

    struct TestDriver;

    #[async_trait]
    impl Driver for TestDriver {
        async fn mount(
            &self,
            _config: &MountConfig,
        ) -> Result<Box<dyn Mount>, MountError> {
            Ok(Box::new(TestMount))
        }
    }

    #[test]
    fn driver_lifecycle_is_callable_through_trait_object() {
        let driver: Box<dyn Driver> = Box::new(TestDriver);
        let config = MountConfig {
            name: "test".to_owned(),
            path: PathBuf::from("/tmp/test"),
            storage: Box::new(EmptyStorageConfig),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("create async runtime");

        let mount = runtime
            .block_on(driver.mount(&config))
            .expect("mount should succeed");
        runtime
            .block_on(driver.unmount(mount.name()))
            .expect("unmount should succeed");
    }
}
