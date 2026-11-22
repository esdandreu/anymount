use std::sync::Arc;

use crate::domain::{
    AuthorizationError, ConnectStorageError, Storage, StorageConfig,
    StorageConnector,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CannotConnectStorageConfig {
    error_message: String,
}

#[typetag::serde(name = "cannot-connect")]
impl StorageConfig for CannotConnectStorageConfig {
    fn create_connector(
        &self,
        _identity: String,
    ) -> Result<Arc<dyn StorageConnector>, AuthorizationError> {
        Ok(Arc::new(self.clone()))
    }
}

#[async_trait::async_trait(?Send)]
impl StorageConnector for CannotConnectStorageConfig {
    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        Err(ConnectStorageError::Failed {
            kind: self.kind(),
            message: self.error_message.clone(),
        })
    }
}
