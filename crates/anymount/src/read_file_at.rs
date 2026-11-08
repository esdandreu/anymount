use std::{ops::Range, path::PathBuf};

use crate::domain::{
    InvalidStoragePath, Storage, StorageError, WriteAt, WriteAtError,
};

/// Reports a file-read operation failure.
#[derive(Debug, thiserror::Error)]
pub enum ReadFileAtError {
    #[error("invalid storage path")]
    InvalidPath(#[from] InvalidStoragePath),

    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Writer(#[from] WriteAtError),
}

/// Reads a file range after validating its storage-relative path.
///
/// # Errors
///
/// Returns [`ReadFileAtError::InvalidPath`] for paths outside the storage
/// root, or forwards errors returned by `storage` or `writer`.
pub async fn read_file_at<S>(
    storage: &S,
    path: PathBuf,
    writer: &mut dyn WriteAt,
    range: Range<u64>,
) -> Result<(), ReadFileAtError>
where
    S: Storage + ?Sized,
{
    if range.start > range.end {
        return Err(StorageError::Failed {
            message: "invalid range".into(),
        }
        .into());
    }
    let path: crate::domain::StoragePath = path.try_into()?;
    let mut pos = range.start;
    while pos < range.end {
        let count = (range.end - pos).min(65536) as usize;
        let bytes = storage.read_range(path.clone(), pos, count).await?;
        if bytes.len() > count {
            return Err(ReadFileAtError::Storage(StorageError::Failed {
                message: "provider returned too much data".into(),
            }));
        }
        if bytes.is_empty() {
            return Err(StorageError::Failed {
                message: "range extends beyond end of file".into(),
            }
            .into());
        }
        writer.write_at(&bytes, pos)?;
        pos += bytes.len() as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::WriteAtError;
    use crate::test_utils::mocks::NoOpStorage;

    struct TestWriter;

    // private test util: Supplies a writer for path validation tests.
    impl WriteAt for TestWriter {
        fn write_at(
            &mut self,
            _buf: &[u8],
            _offset: u64,
        ) -> Result<(), WriteAtError> {
            Ok(())
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_invalid_storage_path() {
        let result = read_file_at(
            &NoOpStorage,
            PathBuf::from("../outside"),
            &mut TestWriter,
            0..1,
        )
        .await;

        assert!(matches!(result, Err(ReadFileAtError::InvalidPath(_))));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn forwards_storage_error() {
        let result =
            read_file_at(&NoOpStorage, PathBuf::new(), &mut TestWriter, 0..1)
                .await;

        assert!(matches!(
            result,
            Err(ReadFileAtError::Storage(StorageError::NotSupported))
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reports_early_end_of_file() {
        let mut storage = crate::domain::MockStorage::new();
        storage.expect_read_range().returning(|_, offset, _| {
            Ok(if offset == 0 { vec![1] } else { vec![] })
        });
        let result = read_file_at(
            &storage,
            PathBuf::from("file"),
            &mut TestWriter,
            0..2,
        )
        .await;
        assert!(matches!(
            result,
            Err(ReadFileAtError::Storage(StorageError::Failed { .. }))
        ));
    }
}
