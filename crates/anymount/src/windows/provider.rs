use crate::{FileMetadata, Result, StorageProvider};
use async_trait::async_trait;
use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Windows Cloud Filter implementation that wraps any StorageProvider
pub struct WindowsCloudProvider {
    inner: Arc<RwLock<Box<dyn StorageProvider>>>,
}

impl WindowsCloudProvider {
    pub fn new(provider: Box<dyn StorageProvider>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(provider)),
        }
    }
}

#[async_trait]
impl StorageProvider for WindowsCloudProvider {
    fn provider_type(&self) -> &str {
        "windows-cloud-filter"
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<FileMetadata>> {
        let provider = self.inner.read().await;
        provider.list_dir(path).await
    }

    async fn get_metadata(&self, path: &str) -> Result<FileMetadata> {
        let provider = self.inner.read().await;
        provider.get_metadata(path).await
    }

    async fn read_file(&self, path: &str) -> Result<Bytes> {
        let provider = self.inner.read().await;
        provider.read_file(path).await
    }

    async fn read_file_range(&self, path: &str, offset: u64, length: u64) -> Result<Bytes> {
        let provider = self.inner.read().await;
        provider.read_file_range(path, offset, length).await
    }

    async fn write_file(&self, path: &str, data: Bytes) -> Result<()> {
        let provider = self.inner.write().await;
        provider.write_file(path, data).await
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        let provider = self.inner.write().await;
        provider.create_dir(path).await
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        let provider = self.inner.write().await;
        provider.delete_file(path).await
    }

    async fn delete_dir(&self, path: &str) -> Result<()> {
        let provider = self.inner.write().await;
        provider.delete_dir(path).await
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        let provider = self.inner.read().await;
        provider.exists(path).await
    }
}

