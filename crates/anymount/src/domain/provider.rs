//! Provider domain types.
//!
//! This module defines provider-facing concepts shared across adapters. The
//! types here describe what a provider is and the invariants it must satisfy
//! before adapter code can persist, mount, or host it.

use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

/// Provider domain validation failures.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Error {
    /// The provider mount path is empty.
    #[error("provider mount path is missing")]
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

/// Result type for provider domain validation.
pub type Result<T> = std::result::Result<T, Error>;

/// A configured provider definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSpec {
    /// Stable provider name derived from config.
    pub name: String,
    /// Local mount path exposed by the provider.
    pub path: PathBuf,
    /// Storage backend configuration for this provider.
    pub storage: StorageSpec,
    /// Optional telemetry configuration for this provider.
    pub telemetry: TelemetrySpec,
}

impl ProviderSpec {
    /// Validates provider invariants.
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
            StorageSpec::OneDrive { endpoint, .. } => Some(endpoint.as_str()),
            StorageSpec::Local { .. } => None,
        }
    }
}

/// Supported storage backends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageSpec {
    /// Local directory storage.
    Local {
        /// Root directory exposed by the provider.
        root: PathBuf,
    },
    /// OneDrive storage via Microsoft Graph.
    OneDrive {
        /// OneDrive path used as the virtual root.
        root: PathBuf,
        /// Microsoft Graph endpoint.
        endpoint: String,
        /// Optional short-lived access token.
        access_token: Option<String>,
        /// Optional refresh token used to obtain new access tokens.
        refresh_token: Option<String>,
        /// Optional OAuth client id override.
        client_id: Option<String>,
        /// Refresh buffer before token expiry.
        token_expiry_buffer_secs: Option<u64>,
    },
}

impl StorageSpec {
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

/// Provider telemetry settings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TelemetrySpec {
    /// Optional OTLP exporter configuration.
    pub otlp: Option<OtlpSpec>,
}

/// OTLP exporter settings for one provider.
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
    use super::{Error, ProviderSpec, StorageSpec, TelemetrySpec};
    use std::path::PathBuf;

    fn local_provider_spec(name: &str) -> ProviderSpec {
        ProviderSpec {
            name: name.to_owned(),
            path: PathBuf::from(format!("/mnt/{name}")),
            storage: StorageSpec::Local {
                root: PathBuf::from(format!("/data/{name}")),
            },
            telemetry: TelemetrySpec::default(),
        }
    }

    #[test]
    fn onedrive_spec_requires_token_material() {
        let spec = ProviderSpec {
            name: "demo".to_owned(),
            path: PathBuf::from("/mnt/demo"),
            storage: StorageSpec::OneDrive {
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
        let spec = local_provider_spec("demo");
        spec.validate().expect("local spec should be valid");
    }
}
