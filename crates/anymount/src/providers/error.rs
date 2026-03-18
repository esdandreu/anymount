#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Storage(#[from] crate::storages::Error),

    #[cfg(target_os = "windows")]
    #[error(transparent)]
    CloudFilter(#[from] crate::providers::cloudfilter::Error),

    #[cfg(target_os = "linux")]
    #[error(transparent)]
    LibCloudProvider(#[from] crate::providers::libcloudprovider::Error),

    #[error("provider runtime not supported on this platform")]
    NotSupported,
}

pub type Result<T> = std::result::Result<T, Error>;
