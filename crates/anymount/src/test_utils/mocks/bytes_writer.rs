#[derive(Default)]
pub struct BytesWriter(pub Vec<u8>);

impl crate::domain::WriteAt for BytesWriter {
    fn write_at(
        &mut self,
        bytes: &[u8],
        _offset: u64,
    ) -> Result<(), crate::domain::WriteAtError> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}
