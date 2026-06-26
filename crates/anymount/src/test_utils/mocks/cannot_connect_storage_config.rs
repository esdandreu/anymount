use crate::domain::{ConnectError, Storage, StorageConfig};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct CannotConnectStorageConfig {
    error_message: String,
}

#[typetag::serde(name = "cannot-connect")]
impl StorageConfig for CannotConnectStorageConfig {
    fn connect(&self) -> Result<Box<dyn Storage>, ConnectError> {
        Err(ConnectError::CannotConnect {
            message: self.error_message.clone(),
        })
    }
}
