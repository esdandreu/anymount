use super::{FileSystemAccessStorage, call};
use crate::domain::{WriteFileError, WriteFileSession};
use async_trait::async_trait;
use futures::{Future, future::poll_fn, io::AsyncWrite};
use js_sys::{Array, Promise, Uint8Array};
use std::cell::Cell;
use std::io;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

#[cfg(test)]
#[path = "browser_write_session_tests.rs"]
mod tests;

// One browser-owned copy bounds memory and amortizes promise overhead.
const WRITE_CHUNK_SIZE: usize = 65536;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Open,
    Sealed,
    Finishing,
    Finished,
    Aborting,
    Aborted,
}

pub struct BrowserWriteSession<'a> {
    storage: &'a FileSystemAccessStorage,
    writable: JsValue,
    pending: Option<JsFuture>,
    state: State,
    failure: Option<WriteFileError>,
}

impl<'a> BrowserWriteSession<'a> {
    pub fn new(
        storage: &'a FileSystemAccessStorage,
        writable: JsValue,
    ) -> Self {
        Self {
            storage,
            writable,
            pending: None,
            state: State::Open,
            failure: None,
        }
    }

    fn start(
        &mut self,
        method: &str,
        args: &Array,
    ) -> Result<(), WriteFileError> {
        let promise = call(&self.writable, method, args)
            .and_then(|value| {
                value.dyn_into::<Promise>().map_err(|_| {
                    JsValue::from_str("browser API did not return a promise")
                })
            })
            .map_err(|error| self.storage.write_error(error))?;
        self.pending = Some(JsFuture::from(promise));
        Ok(())
    }

    fn poll_pending(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), WriteFileError>> {
        let Some(pending) = &mut self.pending else {
            return Poll::Ready(Ok(()));
        };
        let result = std::task::ready!(Pin::new(pending).poll(cx));
        self.pending = None;
        Poll::Ready(
            result
                .map(|_| ())
                .map_err(|error| self.storage.write_error(error)),
        )
    }

    fn remember(&mut self, error: WriteFileError) -> io::Error {
        self.failure = Some(error.clone());
        error.into()
    }
}

impl AsyncWrite for BrowserWriteSession<'_> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if let Some(error) = &self.failure {
            return Poll::Ready(Err(error.clone().into()));
        }
        if self.state != State::Open {
            return Poll::Ready(Err(closed().into()));
        }
        if let Err(error) = std::task::ready!(self.poll_pending(cx)) {
            return Poll::Ready(Err(self.remember(error)));
        }
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let count = bytes.len().min(WRITE_CHUNK_SIZE);
        let chunk = Uint8Array::from(&bytes[..count]);
        if let Err(error) = self.start("write", &Array::of1(&chunk)) {
            return Poll::Ready(Err(self.remember(error)));
        }
        // Accepted bytes belong to the session, not a possibly canceled caller.
        Poll::Ready(Ok(count))
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        if let Some(error) = &self.failure {
            return Poll::Ready(Err(error.clone().into()));
        }
        if !matches!(self.state, State::Open | State::Sealed | State::Finished)
        {
            return Poll::Ready(Err(closed().into()));
        }
        match std::task::ready!(self.poll_pending(cx)) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(error) => Poll::Ready(Err(self.remember(error))),
        }
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        if self.state == State::Open {
            self.state = State::Sealed;
        }
        self.poll_flush(cx)
    }
}

#[async_trait(?Send)]
impl WriteFileSession for BrowserWriteSession<'_> {
    async fn finish(&mut self) -> Result<(), WriteFileError> {
        if self.state == State::Finished {
            return Ok(());
        }
        if matches!(self.state, State::Aborting | State::Aborted) {
            return Err(closed());
        }
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if self.state != State::Finishing {
            self.state = State::Sealed;
            if let Err(error) = poll_fn(|cx| self.poll_pending(cx)).await {
                self.failure = Some(error.clone());
                return Err(error);
            }
            if let Err(error) = self.start("close", &Array::new()) {
                self.failure = Some(error.clone());
                return Err(error);
            }
            self.state = State::Finishing;
        }
        match poll_fn(|cx| self.poll_pending(cx)).await {
            Ok(()) => {
                self.state = State::Finished;
                Ok(())
            }
            Err(error) => {
                self.state = State::Sealed;
                self.failure = Some(error.clone());
                Err(error)
            }
        }
    }

    async fn abort(&mut self) -> Result<(), WriteFileError> {
        if self.state == State::Aborted {
            return Ok(());
        }
        if matches!(self.state, State::Finishing | State::Finished) {
            return Err(closed());
        }
        if self.state != State::Aborting {
            // The browser orders abort after any already-submitted write.
            self.pending = None;
            self.state = State::Sealed;
            self.failure = Some(closed());
            self.start("abort", &Array::new())?;
            self.state = State::Aborting;
        }
        let result = poll_fn(|cx| self.poll_pending(cx)).await;
        self.state = if result.is_ok() {
            State::Aborted
        } else {
            State::Sealed
        };
        result
    }
}

pub struct WorkspaceWriteSession<'a> {
    inner: Box<dyn WriteFileSession + 'a>,
    connected: Rc<Cell<bool>>,
    failure: Option<WriteFileError>,
    sealed: bool,
    finished: bool,
}

impl<'a> WorkspaceWriteSession<'a> {
    pub fn new(
        inner: Box<dyn WriteFileSession + 'a>,
        connected: Rc<Cell<bool>>,
    ) -> Self {
        Self {
            inner,
            connected,
            failure: None,
            sealed: false,
            finished: false,
        }
    }

    fn check(&mut self) -> Result<(), WriteFileError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if !self.connected.get() {
            self.failure = Some(WriteFileError::Disconnected);
            return Err(WriteFileError::Disconnected);
        }
        Ok(())
    }

    fn observe<T>(
        &mut self,
        result: Poll<io::Result<T>>,
    ) -> Poll<io::Result<T>> {
        if let Poll::Ready(Err(error)) = &result {
            if error.kind() == io::ErrorKind::PermissionDenied {
                self.connected.set(false);
            }
            self.failure = Some(WriteFileError::from(error));
        }
        result
    }
}

impl AsyncWrite for WorkspaceWriteSession<'_> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if let Err(error) = self.check() {
            return Poll::Ready(Err(error.into()));
        }
        if self.sealed {
            return Poll::Ready(Err(closed().into()));
        }
        let result = Pin::new(&mut *self.inner).poll_write(cx, bytes);
        self.observe(result)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        if let Err(error) = self.check() {
            return Poll::Ready(Err(error.into()));
        }
        let result = Pin::new(&mut *self.inner).poll_flush(cx);
        self.observe(result)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        self.sealed = true;
        if let Err(error) = self.check() {
            return Poll::Ready(Err(error.into()));
        }
        let result = Pin::new(&mut *self.inner).poll_close(cx);
        self.observe(result)
    }
}

#[async_trait(?Send)]
impl WriteFileSession for WorkspaceWriteSession<'_> {
    async fn finish(&mut self) -> Result<(), WriteFileError> {
        self.sealed = true;
        if self.finished {
            return Ok(());
        }
        self.check()?;
        let result = {
            let connected = &self.connected;
            let mut finish = self.inner.finish();
            poll_fn(|cx| {
                if !connected.get() {
                    return Poll::Ready(Err(WriteFileError::Disconnected));
                }
                finish.as_mut().poll(cx)
            })
            .await
        };
        if let Err(error) = &result {
            if matches!(error, WriteFileError::PermissionDenied) {
                self.connected.set(false);
            }
            self.failure = Some(error.clone());
        } else {
            self.finished = true;
        }
        result
    }

    async fn abort(&mut self) -> Result<(), WriteFileError> {
        self.sealed = true;
        let result = self.inner.abort().await;
        if matches!(result, Err(WriteFileError::PermissionDenied)) {
            self.connected.set(false);
        }
        result
    }
}

fn closed() -> WriteFileError {
    WriteFileError::Failed {
        message: "write session is closed".into(),
    }
}
