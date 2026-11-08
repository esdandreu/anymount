use std::path::PathBuf;

use crate::domain::{InvalidStoragePath, Storage, StorageEntry, StorageError};

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
) -> Result<Vec<StorageEntry>, ReadDirError>
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
    async fn forwards_storage_error() {
        let result = read_dir(&NoOpStorage, PathBuf::new()).await;

        assert!(matches!(
            result,
            Err(ReadDirError::Storage(StorageError::NotSupported))
        ));
    }
}
