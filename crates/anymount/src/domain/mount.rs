use std::path::{Path, PathBuf};
use thiserror::Error;

pub trait Mount: Send + Sync {
    fn name(&self) -> &str;

    // Should it be just Path?
    fn root(&self) -> &Path;

    fn is_connected(&self) -> bool;

    fn connect(&self) -> Result<(), ConnectMountError>;

    fn disconnect(&self) -> Result<(), DisconnectMountError>;

    fn unregister(&self) -> Result<(), UnregisterMountError>;
}

/// A mount could not be connected to its storage.
#[derive(Debug, Error)]
#[error("failed to connect mount at {path}: {message}")]
pub struct ConnectMountError {
    pub path: PathBuf,
    pub message: String,
}

/// A mount could not be disconnected from its storage.
#[derive(Debug, Error)]
#[error("failed to disconnect mount at {path}: {message}")]
pub struct DisconnectMountError {
    pub path: PathBuf,
    pub message: String,
}

/// A mount could not be unregistered.
#[derive(Debug, Error)]
#[error("failed to unregister mount at {path}: {message}")]
pub struct UnregisterMountError {
    pub path: PathBuf,
    pub message: String,
}
