pub mod error;
pub use error::{Error, Result};
pub mod provider;
pub use provider::{StorageProvider, FileMetadata, FileType};

// Provider implementations
pub mod providers;

// Platform-specific modules
#[cfg(target_os = "windows")]
pub mod windows;