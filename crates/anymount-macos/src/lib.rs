/*!
 # anymount-macos

macOS FileProvider integration for anymount.

## Status

Placeholder implementation. FileProvider requires native Swift/Objective-C code.

## Future Implementation

A full FileProvider implementation will require:
- System extension bundle (Swift/Objective-C)
- XPC communication between Rust and the extension
- Code signing and entitlements
- User approval for system extension

For now, this crate provides the macOS-specific mount interface that will
eventually communicate with a FileProvider extension.
*/

#![cfg(target_os = "macos")]

mod bridge;
mod domain;
mod item;

// FFI module for XPC communication
#[cfg(feature = "ffi")]
pub mod ffi;

pub use bridge::FileProviderBridge;
pub use domain::{DomainInfo, FileProviderDomain};
pub use item::{identifier_to_path, path_to_identifier, FileProviderItem};

use anymount_core::{provider::StorageProvider, Result};
use std::sync::Arc;

/// High-level interface for mounting storage providers on macOS
///
/// This struct manages the lifecycle of a FileProvider mount, coordinating
/// between your storage provider and the macOS FileProvider system.
pub struct MacOSMount {
    bridge: FileProviderBridge,
}

impl MacOSMount {
    /// Create a new macOS mount
    ///
    /// # Arguments
    /// * `provider` - The storage provider to mount
    /// * `domain_id` - Unique identifier (e.g., "com.anymount.my-bucket")
    /// * `display_name` - Name shown in Finder (e.g., "My Bucket")
    ///
    /// # Example
    /// ```rust,no_run
    /// use anymount_macos::MacOSMount;
    /// use anymount_providers::MockProvider;
    /// use std::sync::Arc;
    ///
    /// let provider = Arc::new(MockProvider::new());
    /// let mount = MacOSMount::new(
    ///     provider,
    ///     "com.example.storage",
    ///     "My Storage"
    /// )?;
    /// # Ok::<(), anymount_core::Error>(())
    /// ```
    pub fn new(
        provider: Arc<dyn StorageProvider>,
        domain_id: &str,
        display_name: &str,
    ) -> Result<Self> {
        let bridge = FileProviderBridge::new(provider, domain_id, display_name)?;
        Ok(Self { bridge })
    }

    /// Mount the storage provider
    ///
    /// This registers a FileProvider domain with macOS, making it visible in Finder.
    ///
    /// **Note**: This requires a FileProvider extension to be installed. See the
    /// crate documentation for details on creating the extension.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The FileProvider extension is not installed
    /// - The domain is already registered
    /// - System permissions are insufficient
    pub async fn mount(&self) -> Result<()> {
        self.bridge.mount().await
    }

    /// Unmount the storage provider
    ///
    /// This removes the FileProvider domain from the system.
    pub async fn unmount(&self) -> Result<()> {
        self.bridge.unmount().await
    }

    /// Get the bridge for advanced operations
    ///
    /// This provides access to lower-level FileProvider operations like
    /// signaling changes, listing items, etc.
    pub fn bridge(&self) -> &FileProviderBridge {
        &self.bridge
    }

    /// List all registered FileProvider domains
    ///
    /// This returns information about all anymount (and other) FileProvider
    /// domains currently registered with the system.
    pub async fn list_all_mounts() -> Result<Vec<DomainInfo>> {
        FileProviderBridge::list_all_domains().await
    }
}
