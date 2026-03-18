use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Storage(#[from] crate::storages::Error),

    #[error("cache io failed during {operation} for {path}: {source}")]
    CacheIo {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("cache range {start}..{end} is not available for {path}")]
    CacheRangeNotCached { path: PathBuf, start: u64, end: u64 },

    #[error("unexpected eof while reading cache file {path}")]
    CacheUnexpectedEof { path: PathBuf },

    #[error("mount io failed during {operation} for {path}: {source}")]
    MountIo {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("fuse mount failed for {path}: {source}")]
    FuseMount {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("tokio runtime initialization failed: {source}")]
    RuntimeInit {
        #[source]
        source: std::io::Error,
    },

    #[error("d-bus operation {operation} failed: {source}")]
    Dbus {
        operation: &'static str,
        #[source]
        source: zbus::Error,
    },

    #[error("d-bus object registration failed for {object_path} during {operation}: {source}")]
    DbusObject {
        operation: &'static str,
        object_path: String,
        #[source]
        source: zbus::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
