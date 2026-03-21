//! Driver domain types.
//!
//! This module defines driver-facing concepts shared across adapters. The
//! types here describe what a driver is and the invariants it must satisfy
//! before adapter code can persist, mount, or host it.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

/// Driver domain validation failures.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Error {
    /// The driver mount path is empty.
    #[error("driver mount path is missing")]
    MissingMountPath,
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

/// Result type for driver domain validation.
pub type Result<T> = std::result::Result<T, Error>;

/// A configured driver definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverConfig {
    /// Stable driver name derived from config.
    pub name: String,
    /// Local mount path exposed by the driver.
    pub path: PathBuf,
    /// Storage backend configuration for this driver.
    pub storage: StorageConfig,
    /// Optional telemetry configuration for this driver.
    pub telemetry: TelemetrySpec,
}

impl DriverConfig {
    /// Validates driver invariants.
    ///
    /// # Errors
    /// Returns an error when the mount path or storage configuration is
    /// incomplete.
    pub fn validate(&self) -> Result<()> {
        if self.path.as_os_str().is_empty() {
            return Err(Error::MissingMountPath);
        }

        self.storage.validate()
    }

    pub fn onedrive_endpoint(&self) -> Option<&str> {
        match &self.storage {
            StorageConfig::OneDrive { endpoint, .. } => Some(endpoint.as_str()),
            StorageConfig::Local { .. } => None,
        }
    }
}

/// Supported storage backends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StorageConfig {
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

impl StorageConfig {
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
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Local { root } => {
                if root.as_os_str().is_empty() {
                    return Err(Error::MissingLocalRoot);
                }
            }
            Self::OneDrive {
                root,
                access_token,
                refresh_token,
                ..
            } => {
                if root.as_os_str().is_empty() {
                    return Err(Error::MissingOneDriveRoot);
                }

                if access_token.is_none() && refresh_token.is_none() {
                    return Err(Error::MissingOneDriveTokenMaterial);
                }
            }
        }

        Ok(())
    }
}

/// Driver telemetry settings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TelemetrySpec {
    /// Optional OTLP exporter configuration.
    pub otlp: Option<OtlpSpec>,
}

/// OTLP exporter settings for one driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtlpSpec {
    /// Whether OTLP export is enabled.
    pub enabled: bool,
    /// Optional OTLP endpoint override.
    pub endpoint: Option<String>,
    /// Optional transport override.
    pub protocol: Option<OtlpTransport>,
    /// Optional transport headers.
    pub headers: Option<HashMap<String, String>>,
    /// Optional extra resource attributes.
    pub resource_attributes: Option<HashMap<String, String>>,
}

impl Default for OtlpSpec {
    fn default() -> Self {
        Self {
            enabled: true,
            endpoint: None,
            protocol: None,
            headers: None,
            resource_attributes: None,
        }
    }
}

/// OTLP wire transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OtlpTransport {
    /// HTTP/protobuf transport.
    #[default]
    HttpProtobuf,
    /// gRPC transport.
    Grpc,
}

#[cfg(test)]
mod tests {
    use super::{DriverConfig, Error, StorageConfig, TelemetrySpec};
    use std::path::PathBuf;

    fn local_driver_spec(name: &str) -> DriverConfig {
        DriverConfig {
            name: name.to_owned(),
            path: PathBuf::from(format!("/mnt/{name}")),
            storage: StorageConfig::Local {
                root: PathBuf::from(format!("/data/{name}")),
            },
            telemetry: TelemetrySpec::default(),
        }
    }

    #[test]
    fn onedrive_spec_requires_token_material() {
        let spec = DriverConfig {
            name: "demo".to_owned(),
            path: PathBuf::from("/mnt/demo"),
            storage: StorageConfig::OneDrive {
                root: PathBuf::from("/"),
                endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
                access_token: None,
                refresh_token: None,
                client_id: None,
                token_expiry_buffer_secs: Some(60),
            },
            telemetry: TelemetrySpec::default(),
        };

        let err = spec.validate().expect_err("spec should be invalid");
        assert!(matches!(err, Error::MissingOneDriveTokenMaterial));
    }

    #[test]
    fn local_spec_validation_accepts_path_and_root() {
        let spec = local_driver_spec("demo");
        spec.validate().expect("local spec should be valid");
    }
}
