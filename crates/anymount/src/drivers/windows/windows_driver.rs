use super::native_storage::NativeStorage;
use super::windows_mount::WindowsMount;
use crate::domain::{Driver, DriverConfig, MountError};
use crate::domain::{LoadedConfig, Mount};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Configuration for the Windows CloudFilter driver.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct WindowsDriverConfig;

#[typetag::serde(name = "windows")]
impl DriverConfig for WindowsDriverConfig {
    fn init(&self) -> Box<dyn Driver> {
        Box::new(WindowsDriver)
    }
}

inventory::submit! {
    crate::domain::DefaultDriverConfig {
        priority: 0,
        config: &WindowsDriverConfig,
    }
}

/// Creates Windows CloudFilter mounts.
pub struct WindowsDriver;

#[async_trait(?Send)]
impl Driver for WindowsDriver {
    async fn mount(
        &self,
        record: &LoadedConfig,
    ) -> Result<Box<dyn Mount>, MountError> {
        let config = &record.config.mount;
        let session = Arc::clone(&record.session);
        let persistence = record.persistence.clone();
        let storage = tokio::task::spawn_blocking(move || {
            NativeStorage::open(session, persistence)
        })
        .await
        .map_err(|source| MountError::CannotMountAtPath {
            path: config.path.clone(),
            message: source.to_string(),
        })??;
        WindowsMount::new(config.name.clone(), config.path.clone(), storage)
            .map(|mount| Box::new(mount) as Box<dyn Mount>)
            .map_err(|source| MountError::CannotMountAtPath {
                path: config.path.clone(),
                message: source.to_string(),
            })
    }

    async fn unmount(&self, _name: &str) -> Result<(), MountError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{WindowsDriver, WindowsDriverConfig};
    use crate::domain::{Driver, DriverConfig};

    #[test]
    fn config_initializes_empty_driver() {
        let driver = WindowsDriverConfig.init();

        assert_eq!(driver.kind(), std::any::type_name::<WindowsDriver>());
    }
}
