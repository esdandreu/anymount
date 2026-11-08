use crate::domain::{ConnectStorageError, Storage, StorageConfig};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CannotConnectStorageConfig {
    error_message: String,
}

#[typetag::serde(name = "cannot-connect")]
impl StorageConfig for CannotConnectStorageConfig {
    fn clone_config(&self) -> Box<dyn StorageConfig> {
        Box::new(self.clone())
    }
    fn connect(
        &self,
    ) -> crate::domain::StorageFuture<
        '_,
        Result<Box<dyn Storage>, ConnectStorageError>,
    > {
        Box::pin(async {
            Err(ConnectStorageError::Failed {
                kind: self.kind(),
                message: self.error_message.clone(),
            })
        })
    }
}
