use crate::domain::{DirEntry, ReadDirError, Storage, StoragePath};

mockall::mock! {
    pub Storage {
        pub fn read_dir(&self, path: StoragePath) -> Result<Vec<Box<dyn DirEntry>>, ReadDirError>;
    }
}

#[async_trait::async_trait(?Send)]
impl Storage for MockStorage {
    async fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<Vec<Box<dyn DirEntry>>, ReadDirError> {
        MockStorage::read_dir(self, path)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
