use crate::domain::{ConnectStorageError, Storage, StorageConfig};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct CannotConnectStorageConfig {
    error_message: String,
}

#[typetag::serde(name = "cannot-connect")]
impl StorageConfig for CannotConnectStorageConfig {
    fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        Err(ConnectStorageError::Failed {
            kind: self.kind(),
            message: self.error_message.clone(),
        })
    }
}
