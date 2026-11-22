use std::sync::Arc;

use crate::domain::{
    ConnectStorageError, Storage, StorageConfig, StorageConnector,
};

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct EmptyStorageConfig;

#[typetag::serde(name = "empty")]
impl StorageConfig for EmptyStorageConfig {
    fn create_connector(&self, _identity: String) -> Arc<dyn StorageConnector> {
        Arc::new(*self)
    }
}

#[async_trait::async_trait(?Send)]
impl StorageConnector for EmptyStorageConfig {
    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        Ok(Box::new(super::NoOpStorage))
    }
}
