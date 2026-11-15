use std::{ops::Range, path::PathBuf};

use crate::domain::{ReadFileAtError, Storage, WriteAt};

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
        return Err(ReadFileAtError::RangeNotSatisfiable);
    }
    let path: crate::domain::StoragePath = path.try_into()?;
    let expected = range.end - range.start;
    let count = storage.read_file_at(path, writer, range).await?;
    if count != expected {
        return Err(ReadFileAtError::RangeNotSatisfiable);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::mocks::{BytesWriter, NoOpStorage};

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_invalid_storage_path() {
        let result = read_file_at(
            &NoOpStorage,
            PathBuf::from("../outside"),
            &mut BytesWriter::default(),
            0..1,
        )
        .await;

        assert!(matches!(result, Err(ReadFileAtError::InvalidPath(_))));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_reversed_range() {
        let result = read_file_at(
            &NoOpStorage,
            PathBuf::new(),
            &mut BytesWriter::default(),
            Range { start: 2, end: 1 },
        )
        .await;

        assert!(matches!(result, Err(ReadFileAtError::RangeNotSatisfiable)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn forwards_storage_error() {
        let result = read_file_at(
            &NoOpStorage,
            PathBuf::new(),
            &mut BytesWriter::default(),
            0..1,
        )
        .await;

        assert!(matches!(result, Err(ReadFileAtError::NotSupported)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reports_early_end_of_file() {
        let storage = crate::test_utils::mocks::ShortStorage;
        let result = read_file_at(
            &storage,
            PathBuf::from("file"),
            &mut BytesWriter::default(),
            0..2,
        )
        .await;
        assert!(matches!(result, Err(ReadFileAtError::RangeNotSatisfiable)));
    }
}
