pub mod error;
pub mod local;
pub mod onedrive;
pub mod storage;

pub use error::{Error, Result};
pub use local::LocalStorage;
pub use onedrive::{OneDriveConfig, OneDriveStorage};
pub use storage::{DirEntry, Storage, WriteAt};
