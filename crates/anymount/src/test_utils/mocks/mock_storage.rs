use crate::domain::{DirEntry, Storage, StorageError, StoragePath};

mockall::mock! {
    pub Storage {
        pub fn read_dir(&self, path: StoragePath) -> Result<Vec<Box<dyn DirEntry>>, StorageError>;
    }
}

#[async_trait::async_trait(?Send)]
impl Storage for MockStorage {
    async fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<Vec<Box<dyn DirEntry>>, StorageError> {
        MockStorage::read_dir(self, path)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
