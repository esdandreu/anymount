use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("storage io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("unexpected eof while reading {path}")]
    UnexpectedEof { path: PathBuf },

    #[error("write_at failed at offset {offset}: {message}")]
    WriteAt { offset: u64, message: String },

    #[error(transparent)]
    OneDrive(#[from] crate::storages::onedrive::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
