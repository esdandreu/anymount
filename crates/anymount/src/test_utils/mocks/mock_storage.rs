use crate::domain::{DirEntry, DirStream, ReadDirError, Storage, StoragePath};

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
    ) -> Result<DirStream, ReadDirError> {
        MockStorage::read_dir(self, path).map(|entries| {
            Box::pin(futures::stream::iter(entries.into_iter().map(Ok)))
                as DirStream
        })
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
