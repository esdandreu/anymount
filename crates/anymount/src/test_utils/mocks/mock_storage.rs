use crate::domain::{Storage, StorageEntry, StorageError, StoragePath};

mockall::mock! {
    pub Storage {
        pub fn read_dir(&self, path: StoragePath) -> Result<Vec<StorageEntry>, StorageError>;
    }
}

#[async_trait::async_trait(?Send)]
impl Storage for MockStorage {
    async fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<Vec<StorageEntry>, StorageError> {
        MockStorage::read_dir(self, path)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
