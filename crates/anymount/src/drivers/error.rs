#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Storage(#[from] crate::storages::Error),

    #[cfg(target_os = "windows")]
    #[error(transparent)]
    CloudFilter(#[from] crate::drivers::windows::Error),

    #[cfg(target_os = "linux")]
    #[error(transparent)]
    LibCloudProvider(#[from] crate::drivers::linux::Error),

    #[cfg(feature = "fuse")]
    #[error(transparent)]
    Fuse(#[from] crate::drivers::fuse::error::Error),

    #[error("driver runtime not supported on this platform")]
    NotSupported,
}

pub type Result<T> = std::result::Result<T, Error>;
