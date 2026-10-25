use crate::domain::{ConnectStorageError, Storage, StorageConfig};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EmptyStorageConfig;

#[typetag::serde(name = "empty")]
impl StorageConfig for EmptyStorageConfig {
    fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        Ok(Box::new(super::NoOpStorage))
    }
}
