use crate::domain::{WriteAt, WriteAtError};

pub struct CountingWriter {
    pub next: u64,
    pub total: u64,
    pub max_chunk: usize,
    pub fail_after: Option<usize>,
    pub calls: usize,
    pub thread: std::thread::ThreadId,
}

impl CountingWriter {
    pub fn new(start: u64) -> Self {
        Self {
            next: start,
            total: 0,
            max_chunk: 0,
            fail_after: None,
            calls: 0,
            thread: std::thread::current().id(),
        }
    }
}

impl WriteAt for CountingWriter {
    fn write_at(
        &mut self,
        bytes: &[u8],
        offset: u64,
    ) -> Result<(), WriteAtError> {
        assert_eq!(std::thread::current().id(), self.thread);
        assert_eq!(offset, self.next);
        if self.fail_after == Some(self.calls) {
            return Err(WriteAtError::Failed {
                offset,
                message: "test failure".into(),
            });
        }
        self.calls += 1;
        self.max_chunk = self.max_chunk.max(bytes.len());
        self.next += bytes.len() as u64;
        self.total += bytes.len() as u64;
        Ok(())
    }
}
