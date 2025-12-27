use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::Result;
use crate::provider::StorageProvider;

/// Configuration for a mount point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountConfig {
    /// Local path where the storage should be mounted
    pub mount_path: PathBuf,
    
    /// Optional cache directory for downloaded files
    pub cache_dir: Option<PathBuf>,
    
    /// Read-only mount
    pub read_only: bool,
    
    /// Provider-specific configuration
    pub provider_config: ProviderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProviderConfig {
    S3 {
        bucket: String,
        region: String,
        prefix: Option<String>,
    },
    // Future providers
    Azure {
        container: String,
        account: String,
    },
    Gcs {
        bucket: String,
    },
}

/// Represents an active mount point
pub struct MountPoint {
    config: MountConfig,
    provider: Box<dyn StorageProvider>,
}

impl MountPoint {
    pub fn new(config: MountConfig, provider: Box<dyn StorageProvider>) -> Self {
        Self { config, provider }
    }

    pub fn config(&self) -> &MountConfig {
        &self.config
    }

    pub fn provider(&self) -> &dyn StorageProvider {
        self.provider.as_ref()
    }

    /// Mount the storage provider to the configured path
    pub async fn mount(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return crate::mount::macos::mount(self).await;
        
        #[cfg(not(target_os = "macos"))]
        return Err(crate::error::Error::NotSupported(
            "Only macOS is currently supported".into()
        ));
    }

    /// Unmount the storage provider
    pub async fn unmount(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        return crate::mount::macos::unmount(self).await;
        
        #[cfg(not(target_os = "macos"))]
        return Err(crate::error::Error::NotSupported(
            "Only macOS is currently supported".into()
        ));
    }
}

// Platform-specific mount implementation
#[cfg(target_os = "macos")]
pub(crate) mod macos {
    use super::*;
    
    pub async fn mount(_mount_point: &MountPoint) -> Result<()> {
        Err(crate::error::Error::NotSupported(
            "macOS FileProvider implementation pending".into()
        ))
    }
    
    pub async fn unmount(_mount_point: &MountPoint) -> Result<()> {
        Err(crate::error::Error::NotSupported(
            "macOS unmount implementation pending".into()
        ))
    }
}

