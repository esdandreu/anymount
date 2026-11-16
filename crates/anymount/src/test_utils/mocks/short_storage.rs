use crate::domain::{Storage, StoragePath};

pub struct ShortStorage;

#[async_trait::async_trait(?Send)]
impl Storage for ShortStorage {
    async fn read_file_at(
        &self,
        _path: StoragePath,
        _writer: &mut (dyn futures::io::AsyncWrite + Unpin),
        _range: std::ops::Range<u64>,
    ) -> Result<u64, crate::domain::ReadFileAtError> {
        Ok(1)
    }
}
