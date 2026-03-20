use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{operation} failed")]
    WindowsOperation {
        operation: &'static str,
        #[source]
        source: windows::core::Error,
    },

    #[error("{operation} failed for {path}: {source}")]
    WindowsPath {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: windows::core::Error,
    },

    #[error("{operation} failed for {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{operation} failed")]
    CloudFilterOperation {
        operation: &'static str,
        #[source]
        source: windows::core::Error,
    },

    #[error("invalid mount path: {path}")]
    InvalidPath { path: PathBuf },

    #[error("cannot dehydrate folder {path}")]
    CannotDehydrateDirectory { path: PathBuf },
}

pub type Result<T> = std::result::Result<T, Error>;
