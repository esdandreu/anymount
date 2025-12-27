use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Metadata about a file or directory in the storage provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub path: String,
    pub file_type: FileType,
    pub size: u64,
    pub modified: Option<u64>, // Unix timestamp
    pub created: Option<u64>,  // Unix timestamp
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileType {
    File,
    Directory,
}

/// Core trait that all storage providers must implement
#[async_trait]
pub trait StorageProvider: Send + Sync {
    /// Get the name/type of this provider (e.g., "s3", "azure", "gcs")
    fn provider_type(&self) -> &str;

    /// List files in a directory
    async fn list_dir(&self, path: &str) -> Result<Vec<FileMetadata>>;

    /// Get metadata for a specific file/directory
    async fn get_metadata(&self, path: &str) -> Result<FileMetadata>;

    /// Read file contents
    async fn read_file(&self, path: &str) -> Result<Bytes>;

    /// Read a portion of a file
    async fn read_file_range(&self, path: &str, offset: u64, length: u64) -> Result<Bytes>;

    /// Write file contents
    async fn write_file(&self, path: &str, data: Bytes) -> Result<()>;

    /// Create a directory
    async fn create_dir(&self, path: &str) -> Result<()>;

    /// Delete a file
    async fn delete_file(&self, path: &str) -> Result<()>;

    /// Delete a directory
    async fn delete_dir(&self, path: &str) -> Result<()>;

    /// Check if a path exists
    async fn exists(&self, path: &str) -> Result<bool>;
}

