#[cfg(not(target_arch = "wasm32"))]
pub mod google_drive;
#[cfg(not(target_arch = "wasm32"))]
pub mod local;
#[cfg(not(target_arch = "wasm32"))]
pub mod onedrive;

#[cfg(not(target_arch = "wasm32"))]
pub use local::{LocalDirEntry, LocalStorage};
#[cfg(not(target_arch = "wasm32"))]
pub use onedrive::{OneDriveConfig, OneDriveDirEntry, OneDriveStorage};

#[cfg(not(target_arch = "wasm32"))]
pub use google_drive::{GoogleDriveConfig, GoogleDriveStorage};

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[path = "test_utils/mocks/mock_drive_http.rs"]
pub mod mock_drive_http;
