pub mod error;
pub mod local;
pub mod onedrive;
pub mod storage;

pub use error::{Error, Result};
pub use local::{LocalDirEntry, LocalStorage};
pub use onedrive::{OneDriveConfig, OneDriveDirEntry, OneDriveStorage};
pub use storage::{DirEntry, Storage, StorageConfig, WriteAt};
