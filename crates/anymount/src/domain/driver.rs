use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

use super::{Mount, Storage};

#[typetag::serde(tag = "type")]
pub trait DriverConfig: Send + Sync {
    fn init(&self) -> Box<dyn Driver>;

    fn kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

pub trait Driver: Send + Sync {
    /// Registers a mount. If a mount was already registered, with the same
    /// name and path the storage will be re-configured.
    fn mount(
        &self,
        name: String,
        path: PathBuf,
        storage: Box<dyn Storage>,
    ) -> Result<Box<dyn Mount>, MountError>;

    fn kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Lists registered mounts.
    fn list_mounts(
        &self,
    ) -> Result<Box<dyn Iterator<Item = Box<dyn Mount>>>, ListMountsError>;
}

#[derive(Debug, thiserror::Error)]
pub enum MountError {
    /// A path conflict. Typically the path is already mounted or not accessible
    /// in some way.
    #[error("failed to mount at {path}: {message}")]
    CannotMountAtPath { path: PathBuf, message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ListMountsError {
    #[error("failed to list mounts: {message}")]
    CannotListMounts { message: String },
}

/// Driver domain validation failures.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LegacyError {
    /// The local storage root is empty.
    #[error("local storage root is missing")]
    MissingLocalRoot,
    /// The OneDrive root is empty.
    #[error("OneDrive root is missing")]
    MissingOneDriveRoot,
    /// The OneDrive config has no access or refresh token.
    #[error("OneDrive token material is missing")]
    MissingOneDriveTokenMaterial,
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

/// Result type for driver domain validation.
pub type LegacyResult<T> = std::result::Result<T, LegacyError>;

/// Supported storage backends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LegacyStorageConfig {
    /// Local directory storage.
    Local {
        /// Root directory exposed by the driver.
        root: PathBuf,
    },
    /// OneDrive storage via Microsoft Graph.
    OneDrive {
        /// OneDrive path used as the virtual root.
        root: PathBuf,
        /// Microsoft Graph endpoint.
        endpoint: String,
        /// Optional short-lived access token.
        #[serde(skip_serializing_if = "Option::is_none")]
        access_token: Option<String>,
        /// Optional refresh token used to obtain new access tokens.
        #[serde(skip_serializing_if = "Option::is_none")]
        refresh_token: Option<String>,
        /// Optional OAuth client id override.
        #[serde(skip_serializing_if = "Option::is_none")]
        client_id: Option<String>,
        /// Refresh buffer before token expiry.
        #[serde(skip_serializing_if = "Option::is_none")]
        token_expiry_buffer_secs: Option<u64>,
    },
}

impl LegacyStorageConfig {
    /// Short label for CLI and status output (`local`, `onedrive`, ...).
    pub fn label(&self) -> &'static str {
        match self {
            Self::Local { .. } => "local",
            Self::OneDrive { .. } => "onedrive",
        }
    }

    /// Validates storage-specific invariants.
    ///
    /// # Errors
    /// Returns an error when the storage config is missing a required path or
    /// token.
    pub fn validate(&self) -> LegacyResult<()> {
        match self {
            Self::Local { root } => {
                if root.as_os_str().is_empty() {
                    return Err(LegacyError::MissingLocalRoot);
                }
            }
            Self::OneDrive {
                root,
                access_token,
                refresh_token,
                ..
            } => {
                if root.as_os_str().is_empty() {
                    return Err(LegacyError::MissingOneDriveRoot);
                }

                if access_token.is_none() && refresh_token.is_none() {
                    return Err(LegacyError::MissingOneDriveTokenMaterial);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{LegacyError, LegacyStorageConfig};
    use std::path::PathBuf;

    #[test]
    fn onedrive_spec_requires_token_material() {
        let storage = LegacyStorageConfig::OneDrive {
            root: PathBuf::from("/"),
            endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
            access_token: None,
            refresh_token: None,
            client_id: None,
            token_expiry_buffer_secs: Some(60),
        };

        let err = storage.validate().expect_err("storage should be invalid");
        assert!(matches!(err, LegacyError::MissingOneDriveTokenMaterial));
    }

    #[test]
    fn local_storage_validation_accepts_root() {
        let storage = LegacyStorageConfig::Local {
            root: PathBuf::from("/data/demo"),
        };
        storage.validate().expect("local storage should be valid");
    }
}
