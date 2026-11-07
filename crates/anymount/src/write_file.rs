use std::{io::Read, path::PathBuf};

use crate::domain::{
    InvalidStoragePath, Storage, WriteFileError as StorageError,
};

/// Reports a file-write operation failure.
#[derive(Debug, thiserror::Error)]
pub enum WriteFileError {
    #[error("invalid storage path")]
    InvalidPath(#[from] InvalidStoragePath),

    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Creates or replaces a file after validating its storage-relative path.
///
/// Reads exactly `length` bytes from `reader`. A failed write may leave the
/// destination changed.
///
/// # Errors
///
/// Returns an invalid-path error or forwards the storage write error.
pub fn write_file<S>(
    storage: &S,
    path: PathBuf,
    reader: &mut dyn Read,
    length: u64,
) -> Result<(), WriteFileError>
where
    S: Storage + ?Sized,
{
    Ok(storage.write_file(path.try_into()?, reader, length)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::MockStorage, test_utils::mocks::NoOpStorage};

    #[test]
    fn rejects_invalid_storage_path_before_reading() {
        let mut input = &b"data"[..];
        let result = write_file(
            &NoOpStorage,
            PathBuf::from("../outside"),
            &mut input,
            4,
        );

        assert!(matches!(result, Err(WriteFileError::InvalidPath(_))));
        assert_eq!(input, b"data");
    }

    #[test]
    fn forwards_storage_error() {
        let result = write_file(
            &NoOpStorage,
            PathBuf::from("file"),
            &mut &b"data"[..],
            4,
        );

        assert!(matches!(
            result,
            Err(WriteFileError::Storage(StorageError::NotSupported))
        ));
    }

    #[test]
    fn forwards_input_error() {
        let mut storage = MockStorage::new();
        storage.expect_write_file().returning(|_, _, _| {
            Err(StorageError::Input(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "short input",
            )))
        });

        let result =
            write_file(&storage, PathBuf::from("file"), &mut &b""[..], 1);

        assert!(matches!(
            result,
            Err(WriteFileError::Storage(StorageError::Input(error)))
                if error.kind() == std::io::ErrorKind::UnexpectedEof
        ));
    }

    #[test]
    fn forwards_arguments_through_boxed_storage() {
        let mut storage = MockStorage::new();
        storage.expect_write_file().times(1).returning(
            |path, reader, length| {
                assert_eq!(path.as_path(), std::path::Path::new("nested/file"));
                assert_eq!(length, 4);
                let mut content = [0; 4];
                reader.read_exact(&mut content).expect("read input");
                assert_eq!(&content, b"data");
                Ok(())
            },
        );
        let storage: Box<dyn Storage> = Box::new(storage);
        let mut input = &b"dataextra"[..];

        write_file(&storage, PathBuf::from("nested/file"), &mut input, 4)
            .expect("write succeeds");
        assert_eq!(input, b"extra");
    }
}
