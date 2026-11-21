use std::sync::Arc;

use crate::domain::{
    AuthorizationError, ConnectStorageError, Storage, StorageConfig,
    StorageSession,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CannotConnectStorageConfig {
    error_message: String,
}

#[typetag::serde(name = "cannot-connect")]
impl StorageConfig for CannotConnectStorageConfig {
    fn open_session(
        &self,
        _identity: String,
    ) -> Result<Arc<dyn StorageSession>, AuthorizationError> {
        Ok(Arc::new(self.clone()))
    }
}

#[async_trait::async_trait(?Send)]
impl StorageSession for CannotConnectStorageConfig {
    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        Err(ConnectStorageError::Failed {
            kind: self.kind(),
            message: self.error_message.clone(),
        })
    }
}
