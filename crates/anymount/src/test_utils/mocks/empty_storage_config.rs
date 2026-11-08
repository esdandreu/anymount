use crate::domain::{ConnectStorageError, Storage, StorageConfig};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct EmptyStorageConfig;

#[typetag::serde(name = "empty")]
#[async_trait::async_trait(?Send)]
impl StorageConfig for EmptyStorageConfig {
    fn clone_config(&self) -> Box<dyn StorageConfig> {
        Box::new(self.clone())
    }
    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        Ok(Box::new(super::NoOpStorage))
    }
}
