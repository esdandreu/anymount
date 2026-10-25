use super::windows_mount::WindowsMount;
use crate::domain::{Driver, DriverConfig, MountError};
use crate::domain::{Mount, MountConfig};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

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

#[async_trait]
impl Driver for WindowsDriver {
    async fn mount(
        &self,
        config: &MountConfig,
    ) -> std::result::Result<Box<dyn Mount>, MountError> {
        let storage = config.storage.connect()?;
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
