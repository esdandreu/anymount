use std::sync::OnceLock;

use crate::domain::DriverConfig;
use crate::domain::driver::DefaultDriverConfig;

/// Memoized default driver configuration.
static DEFAULT_DRIVER_CONFIG: OnceLock<Option<&'static dyn DriverConfig>> =
    OnceLock::new();

pub fn get_default_driver_config()
-> Result<&'static dyn DriverConfig, NoDefaultDriver> {
    DEFAULT_DRIVER_CONFIG
        .get_or_init(|| {
            inventory::iter::<DefaultDriverConfig>
                .into_iter()
                .max_by_key(|entry| entry.priority)
                .map(|entry| entry.config)
        })
        .as_ref()
        .copied()
        .ok_or(NoDefaultDriver)
}

#[derive(Debug, thiserror::Error)]
#[error("no default driver is available")]
pub struct NoDefaultDriver;
