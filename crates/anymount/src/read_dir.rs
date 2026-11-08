use std::path::PathBuf;

use crate::domain::{DirEntry, InvalidStoragePath, Storage, StorageError};

/// Reports a directory-read operation failure.
#[derive(Debug, thiserror::Error)]
pub enum ReadDirError {
    #[error("invalid storage path")]
    InvalidPath(#[from] InvalidStoragePath),

    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Native(#[from] crate::domain::ReadDirError),
}

/// Lists a directory after validating its storage-relative path.
///
/// # Errors
///
/// Returns [`ReadDirError::InvalidPath`] for paths outside the storage root,
/// or forwards errors returned by `storage`.
pub async fn read_dir<S>(
    storage: &S,
    path: PathBuf,
) -> Result<Vec<Box<dyn DirEntry>>, ReadDirError>
where
    S: Storage + ?Sized,
{
    Ok(storage.read_dir(path.try_into()?).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::mocks::NoOpStorage;

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_invalid_storage_path() {
        let result = read_dir(&NoOpStorage, PathBuf::from("../outside")).await;

        assert!(matches!(result, Err(ReadDirError::InvalidPath(_))));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn returns_entries_through_storage_trait_object() {
        let mut storage = crate::test_utils::mocks::MockStorage::new();
        storage.expect_read_dir().returning(|_| {
            Ok(vec![Box::new(crate::StorageEntry {
                name: "note".into(),
                is_directory: false,
                size: None,
                accessed: None,
            })])
        });
        let storage: Box<dyn Storage> = Box::new(storage);
        let entries = read_dir(&*storage, PathBuf::new()).await.expect("list");
        assert_eq!(entries[0].file_name(), "note");
        assert!(!entries[0].is_dir());
        assert_eq!(entries[0].size(), None);
        assert_eq!(entries[0].accessed(), None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn forwards_storage_error() {
        let result = read_dir(&NoOpStorage, PathBuf::new()).await;

        assert!(matches!(
            result,
            Err(ReadDirError::Storage(StorageError::NotSupported))
        ));
    }
}
