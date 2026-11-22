#[cfg(not(target_arch = "wasm32"))]
pub mod local;
pub mod onedrive;

#[cfg(not(target_arch = "wasm32"))]
pub use local::{LocalDirEntry, LocalStorage};
pub use onedrive::{
    OneDriveConfig, OneDriveConnector, OneDriveDirEntry, OneDriveStorage,
};
