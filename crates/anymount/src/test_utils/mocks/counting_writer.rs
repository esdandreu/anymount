use futures::io::AsyncWrite;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

#[derive(Debug)]
pub struct CountingWriter {
    pub total: u64,
    pub max_chunk: usize,
    pub fail_after: Option<usize>,
    pub calls: usize,
    pub thread: std::thread::ThreadId,
}

impl CountingWriter {
    pub fn new() -> Self {
        Self {
            total: 0,
            max_chunk: 0,
            fail_after: None,
            calls: 0,
            thread: std::thread::current().id(),
        }
    }
}

impl AsyncWrite for CountingWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        assert_eq!(std::thread::current().id(), self.thread);
        if self.fail_after == Some(self.calls) {
            return Poll::Ready(Err(io::Error::other("test failure")));
        }
        self.calls += 1;
        self.max_chunk = self.max_chunk.max(bytes.len());
        self.total += bytes.len() as u64;
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        panic!("range reads must not flush the sink")
    }

    fn poll_close(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        panic!("range reads must not close the sink")
    }
}

impl Default for CountingWriter {
    fn default() -> Self {
        Self::new()
    }
}
