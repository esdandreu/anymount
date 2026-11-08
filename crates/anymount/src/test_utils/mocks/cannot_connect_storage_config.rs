use crate::domain::{ConnectStorageError, Storage, StorageConfig};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CannotConnectStorageConfig {
    error_message: String,
}

#[typetag::serde(name = "cannot-connect")]
#[async_trait::async_trait(?Send)]
impl StorageConfig for CannotConnectStorageConfig {
    fn clone_config(&self) -> Box<dyn StorageConfig> {
        Box::new(self.clone())
    }
    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        Err(ConnectStorageError::Failed {
            kind: self.kind(),
            message: self.error_message.clone(),
        })
    }
}
