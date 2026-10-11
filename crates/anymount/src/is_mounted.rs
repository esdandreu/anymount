//! Determines whether a configured mount is registered.

use crate::domain::{Config, MountStatusError};
use crate::drivers::{NoDefaultDriver, init_driver};

pub use crate::domain::MountStatus;

/// Determines the registration status of `config`.
///
/// # Errors
///
/// Returns an error when no driver is configured or the mount cannot be
/// inspected.
pub fn is_mounted(config: Config) -> Result<MountStatus, IsMountedError> {
    Ok(init_driver(config.driver.as_deref())?.is_mounted(&config.mount)?)
}

/// An error encountered while determining a mount's status.
#[derive(Debug, thiserror::Error)]
pub enum IsMountedError {
    /// No configured or platform-default driver is available.
    #[error(transparent)]
    DriverSelection(#[from] NoDefaultDriver),

    /// The driver could not inspect the mount registration.
    #[error(transparent)]
    CannotInspectMount(#[from] MountStatusError),
}
