#[cfg(not(target_arch = "wasm32"))]
pub mod gdrive;
#[cfg(not(target_arch = "wasm32"))]
pub mod local;
#[cfg(not(target_arch = "wasm32"))]
pub mod onedrive;

#[cfg(not(target_arch = "wasm32"))]
pub use local::{LocalDirEntry, LocalStorage};
#[cfg(not(target_arch = "wasm32"))]
pub use onedrive::{OneDriveConfig, OneDriveDirEntry, OneDriveStorage};

#[cfg(not(target_arch = "wasm32"))]
pub use gdrive::{GoogleDriveConfig, GoogleDriveStorage};
