use std::{ops::Range, path::PathBuf};

use crate::domain::{
    InvalidStoragePath, ReadFileAtError as StorageError, Storage, WriteAt,
};

/// Reports a file-read operation failure.
#[derive(Debug, thiserror::Error)]
pub enum ReadFileAtError {
    #[error("invalid storage path")]
    InvalidPath(#[from] InvalidStoragePath),

    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Reads a file range after validating its storage-relative path.
///
/// # Errors
///
/// Returns [`ReadFileAtError::InvalidPath`] for paths outside the storage
/// root, or forwards errors returned by `storage` or `writer`.
pub fn read_file_at<S>(
    storage: &S,
    path: PathBuf,
    writer: &mut dyn WriteAt,
    range: Range<u64>,
) -> Result<(), ReadFileAtError>
where
    S: Storage + ?Sized,
{
    Ok(storage.read_file_at(path.try_into()?, writer, range)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::WriteAtError;
    use crate::test_utils::mocks::NoOpStorage;

    struct TestWriter;

    impl WriteAt for TestWriter {
        fn write_at(
            &mut self,
            _buf: &[u8],
            _offset: u64,
        ) -> Result<(), WriteAtError> {
            Ok(())
        }
    }

    #[test]
    fn rejects_invalid_storage_path() {
        let result = read_file_at(
            &NoOpStorage,
            PathBuf::from("../outside"),
            &mut TestWriter,
            0..1,
        );

        assert!(matches!(result, Err(ReadFileAtError::InvalidPath(_))));
    }

    #[test]
    fn forwards_storage_error() {
        let result =
            read_file_at(&NoOpStorage, PathBuf::new(), &mut TestWriter, 0..1);

        assert!(matches!(
            result,
            Err(ReadFileAtError::Storage(StorageError::NotSupported))
        ));
    }
}
