#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[cfg(target_os = "windows")]
    #[error(transparent)]
    CloudFilter(#[from] crate::drivers::windows::Error),

    #[cfg(feature = "fuse")]
    #[error(transparent)]
    Fuse(#[from] crate::drivers::fuse::error::Error),

    #[error("driver runtime not supported on this platform")]
    NotSupported,

    #[error("failed to connect legacy storage: {message}")]
    LegacyStorage { message: String },
}

pub type Result<T> = std::result::Result<T, Error>;
