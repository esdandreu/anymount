pub mod local;
pub mod onedrive;

pub use local::{LocalDirEntry, LocalStorage};
pub use onedrive::{OneDriveConfig, OneDriveDirEntry, OneDriveStorage};
