use crate::domain::{ConnectStorageError, Storage, StorageConfig};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct EmptyStorageConfig;

#[typetag::serde(name = "empty")]
impl StorageConfig for EmptyStorageConfig {
    fn clone_config(&self) -> Box<dyn StorageConfig> {
        Box::new(self.clone())
    }
    fn connect(
        &self,
    ) -> crate::domain::StorageFuture<
        '_,
        Result<Box<dyn Storage>, ConnectStorageError>,
    > {
        Box::pin(async { Ok(Box::new(super::NoOpStorage) as Box<dyn Storage>) })
    }
}
