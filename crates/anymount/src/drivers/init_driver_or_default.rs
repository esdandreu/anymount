use std::sync::OnceLock;

use crate::domain::driver::DefaultDriverConfig;
use crate::domain::{Driver, DriverConfig};

/// Initializes the configured driver or the default driver.
pub fn init_driver_or_default(
    config: Option<Box<dyn DriverConfig>>,
) -> Result<Box<dyn Driver>, NoDefaultDriver> {
    config.map_or_else(
        || get_default_driver().ok_or(NoDefaultDriver),
        |driver| Ok(driver.init()),
    )
}

/// Indicates that no default driver is available.
#[derive(Debug, thiserror::Error)]
#[error("no default driver is available")]
pub struct NoDefaultDriver;

/// Memoized default driver configuration for `get_default_driver`.
static DEFAULT_DRIVER_CONFIG: OnceLock<Option<&'static dyn DriverConfig>> =
    OnceLock::new();

/// Creates the default driver if one is available.
pub fn get_default_driver() -> Option<Box<dyn Driver>> {
    DEFAULT_DRIVER_CONFIG
        .get_or_init(|| {
            inventory::iter::<DefaultDriverConfig>
                .into_iter()
                .max_by_key(|entry| entry.priority)
                .map(|entry| entry.config)
        })
        .map(|config| config.init())
}
