use crate::domain::{Storage, StoragePath, WriteFileError, WriteSession};
use async_trait::async_trait;
use futures::{
    future::poll_fn,
    io::{AsyncWrite, AsyncWriteExt},
};
use std::{
    cell::RefCell,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[derive(Debug, Default)]
pub struct WriteSessionState {
    pub staged: Vec<u8>,
    pub committed: Option<Vec<u8>>,
    pub events: Vec<&'static str>,
    pub delay_write: bool,
    pub delay_finish: bool,
    pub delay_abort: bool,
    pub fail_write: bool,
    pub fail_finish: bool,
    pub fail_abort: bool,
    waker: Option<Waker>,
}

impl WriteSessionState {
    pub fn release(&mut self) {
        self.delay_write = false;
        self.delay_finish = false;
        self.delay_abort = false;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

#[derive(Debug)]
pub struct MemoryWriteStorage<'a>(pub &'a Rc<RefCell<WriteSessionState>>);

#[async_trait(?Send)]
impl Storage for MemoryWriteStorage<'_> {
    async fn open_write(
        &self,
        _: StoragePath,
    ) -> Result<Box<dyn WriteSession + '_>, WriteFileError> {
        Ok(Box::new(MemoryWriteSession {
            state: self.0,
            sealed: false,
            failed: false,
            finished: false,
            aborted: false,
        }))
    }
}

struct MemoryWriteSession<'a> {
    state: &'a Rc<RefCell<WriteSessionState>>,
    sealed: bool,
    failed: bool,
    finished: bool,
    aborted: bool,
}

impl AsyncWrite for MemoryWriteSession<'_> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.sealed || self.failed {
            return Poll::Ready(Err(failure().into()));
        }
        if self.state.borrow().fail_write {
            self.failed = true;
            return Poll::Ready(Err(failure().into()));
        }
        if self.state.borrow().delay_write {
            self.state.borrow_mut().waker = Some(cx.waker().clone());
            return Poll::Pending;
        }
        let count = bytes.len().min(2);
        let mut state = self.state.borrow_mut();
        state.events.push("write");
        state.staged.extend_from_slice(&bytes[..count]);
        Poll::Ready(Ok(count))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        if self.failed || self.aborted {
            return Poll::Ready(Err(failure().into()));
        }
        self.state.borrow_mut().events.push("flush");
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        self.sealed = true;
        self.poll_flush(cx)
    }
}

#[async_trait(?Send)]
impl WriteSession for MemoryWriteSession<'_> {
    async fn finish(&mut self) -> Result<(), WriteFileError> {
        if self.finished {
            return Ok(());
        }
        if self.failed || self.aborted {
            return Err(failure());
        }
        self.sealed = true;
        poll_fn(|cx| {
            let mut state = self.state.borrow_mut();
            if state.delay_finish {
                state.waker = Some(cx.waker().clone());
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;
        if self.state.borrow().fail_finish {
            self.failed = true;
            return Err(failure());
        }
        let mut state = self.state.borrow_mut();
        state.events.push("finish");
        state.committed = Some(state.staged.clone());
        self.finished = true;
        Ok(())
    }

    async fn abort(&mut self) -> Result<(), WriteFileError> {
        if self.finished {
            return Err(failure());
        }
        if self.aborted {
            return Ok(());
        }
        self.sealed = true;
        self.failed = true;
        poll_fn(|cx| {
            let mut state = self.state.borrow_mut();
            if state.delay_abort {
                state.waker = Some(cx.waker().clone());
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;
        if self.state.borrow().fail_abort {
            return Err(failure());
        }
        let mut state = self.state.borrow_mut();
        state.events.push("abort");
        state.staged.clear();
        self.aborted = true;
        Ok(())
    }
}

pub struct TrailerWriteSession<'a> {
    inner: Box<dyn WriteSession + 'a>,
    trailer_offset: usize,
    sealed: bool,
    failed: bool,
    finished: bool,
    pub fail_finalize: bool,
}

impl<'a> TrailerWriteSession<'a> {
    pub fn new(inner: Box<dyn WriteSession + 'a>) -> Self {
        Self {
            inner,
            trailer_offset: 0,
            sealed: false,
            failed: false,
            finished: false,
            fail_finalize: false,
        }
    }
}

impl AsyncWrite for TrailerWriteSession<'_> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.sealed || self.failed {
            return Poll::Ready(Err(failure().into()));
        }
        let result = Pin::new(&mut *self.inner).poll_write(cx, bytes);
        if matches!(result, Poll::Ready(Err(_))) {
            self.failed = true;
        }
        result
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        if self.failed {
            return Poll::Ready(Err(failure().into()));
        }
        let result = Pin::new(&mut *self.inner).poll_flush(cx);
        if matches!(result, Poll::Ready(Err(_))) {
            self.failed = true;
        }
        result
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        self.sealed = true;
        self.poll_flush(cx)
    }
}

#[async_trait(?Send)]
impl WriteSession for TrailerWriteSession<'_> {
    async fn finish(&mut self) -> Result<(), WriteFileError> {
        if self.finished {
            return Ok(());
        }
        if self.failed {
            return Err(failure());
        }
        self.sealed = true;
        if self.fail_finalize {
            self.failed = true;
            return Err(failure());
        }
        let trailer = b"<end>";
        while self.trailer_offset < trailer.len() {
            match self.inner.write(&trailer[self.trailer_offset..]).await {
                Ok(0) => {
                    self.failed = true;
                    return Err(failure());
                }
                Ok(count) => self.trailer_offset += count,
                Err(error) => {
                    self.failed = true;
                    return Err(error.into());
                }
            }
        }
        if let Err(error) = self.inner.finish().await {
            self.failed = true;
            return Err(error);
        }
        self.finished = true;
        Ok(())
    }

    async fn abort(&mut self) -> Result<(), WriteFileError> {
        self.failed = true;
        self.sealed = true;
        self.inner.abort().await
    }
}

fn failure() -> WriteFileError {
    WriteFileError::Failed {
        message: "injected write session failure".into(),
    }
}
