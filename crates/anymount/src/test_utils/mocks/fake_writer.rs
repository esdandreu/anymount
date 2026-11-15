use futures::io::AsyncWrite;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

#[derive(Debug)]
pub struct FakeWriter {
    pub bytes: Vec<u8>,
    pub max_write: usize,
    pub pending: bool,
    pub pending_polls: usize,
    pub fail_after: Option<usize>,
    pub zero_after: Option<usize>,
    pub flushes: usize,
    pub closes: usize,
    ready: bool,
}

impl FakeWriter {
    pub fn new() -> Self {
        Self {
            bytes: Vec::new(),
            max_write: usize::MAX,
            pending: false,
            pending_polls: 0,
            fail_after: None,
            zero_after: None,
            flushes: 0,
            closes: 0,
            ready: false,
        }
    }

    pub fn total_bytes(&self) -> u64 {
        self.bytes.len() as u64
    }

    pub fn flat_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl AsyncWrite for FakeWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.pending && !self.ready {
            self.ready = true;
            self.pending_polls += 1;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        self.ready = false;
        Poll::Ready(io::Write::write(&mut *self, bytes))
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(io::Write::flush(&mut *self))
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        self.closes += 1;
        Poll::Ready(Ok(()))
    }
}

impl io::Write for FakeWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .fail_after
            .is_some_and(|limit| self.bytes.len() >= limit)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "test sink failure",
            ));
        }
        let mut count = bytes.len().min(self.max_write);
        if let Some(limit) = self.fail_after {
            count = count.min(limit.saturating_sub(self.bytes.len()));
        }
        if let Some(limit) = self.zero_after {
            count = count.min(limit.saturating_sub(self.bytes.len()));
        }
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

impl Default for FakeWriter {
    fn default() -> Self {
        Self::new()
    }
}
