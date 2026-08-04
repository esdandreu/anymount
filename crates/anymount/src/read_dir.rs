use std::path::PathBuf;

use crate::domain::storage::{
    DirEntry, InvalidStoragePath, ReadDirError as StorageError, Storage,
};

/// Reports a directory-read operation failure.
#[derive(Debug, thiserror::Error)]
pub enum ReadDirError {
    #[error("invalid storage path")]
    InvalidPath(#[from] InvalidStoragePath),

    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Lists a directory after validating its storage-relative path.
///
/// # Errors
///
/// Returns [`ReadDirError::InvalidPath`] for paths outside the storage root,
/// or forwards errors returned by `storage`.
pub fn read_dir<S>(
    storage: &S,
    path: PathBuf,
) -> Result<Box<dyn Iterator<Item = Box<dyn DirEntry>>>, ReadDirError>
where
    S: Storage + ?Sized,
{
    Ok(storage.read_dir(path.try_into()?)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::mocks::MockStorage;

    #[test]
    fn rejects_invalid_storage_path() {
        let result = read_dir(&MockStorage, PathBuf::from("../outside"));

        assert!(matches!(result, Err(ReadDirError::InvalidPath(_))));
    }

    #[test]
    fn forwards_storage_error() {
        let result = read_dir(&MockStorage, PathBuf::new());

        assert!(matches!(
            result,
            Err(ReadDirError::Storage(StorageError::NotSupported))
        ));
    }
}
