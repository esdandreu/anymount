use async_trait::async_trait;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[async_trait]
pub trait Mount: Send + Sync {
    fn name(&self) -> &str;

    // Should it be just Path?
    fn root(&self) -> &Path;

    fn is_connected(&self) -> bool;

    async fn connect(&self) -> Result<(), ConnectMountError>;

    async fn disconnect(&self) -> Result<(), DisconnectMountError>;

    async fn unregister(&self) -> Result<(), UnregisterMountError>;
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
