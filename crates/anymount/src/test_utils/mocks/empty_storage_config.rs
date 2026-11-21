use std::sync::Arc;

use crate::domain::{
    AuthorizationError, ConnectStorageError, Storage, StorageConfig,
    StorageSession,
};

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct EmptyStorageConfig;

#[typetag::serde(name = "empty")]
impl StorageConfig for EmptyStorageConfig {
    fn open_session(
        &self,
        _identity: String,
    ) -> Result<Arc<dyn StorageSession>, AuthorizationError> {
        Ok(Arc::new(*self))
    }
}

#[async_trait::async_trait(?Send)]
impl StorageSession for EmptyStorageConfig {
    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        Ok(Box::new(super::NoOpStorage))
    }
}
