//! Determines whether a configured mount is registered.

use crate::domain::{Config, MountStatusError};
use crate::drivers::{NoDefaultDriver, init_driver_or_default};

pub use crate::domain::MountStatus;

/// Determines the registration status of `config`.
///
/// # Errors
///
/// Returns an error when no driver is configured or the mount cannot be
/// inspected.
pub fn is_mounted(config: Config) -> Result<MountStatus, IsMountedError> {
    let Config {
        name, path, driver, ..
    } = config;
    Ok(init_driver_or_default(driver)?.is_mounted(&name, &path)?)
}

/// An error encountered while determining a mount's status.
#[derive(Debug, thiserror::Error)]
pub enum IsMountedError {
    /// No configured or platform-default driver is available.
    #[error(transparent)]
    NoDefaultDriver(#[from] NoDefaultDriver),

    /// The driver could not inspect the mount registration.
    #[error(transparent)]
    CannotInspectMount(#[from] MountStatusError),
}
