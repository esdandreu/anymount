use super::get_default_driver_config::{
    NoDefaultDriver, get_default_driver_config,
};
use crate::domain::{Driver, DriverConfig};

/// Initializes the configured driver or the default driver.
pub fn init_driver(
    config: Option<&dyn DriverConfig>,
) -> Result<Box<dyn Driver>, NoDefaultDriver> {
    let config = match config {
        Some(config) => config,
        None => get_default_driver_config()?,
    };
    Ok(config.init())
}
