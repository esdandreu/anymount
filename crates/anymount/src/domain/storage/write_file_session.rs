use async_trait::async_trait;
use futures::io::AsyncWrite;

/// Writes a file incrementally with explicit commit and cancellation.
///
/// `AsyncWrite::poll_flush` drains buffered output without committing.
/// `AsyncWrite::poll_close` seals input and flushes, but does not commit.
/// After close, `finish` must still finalize transforms before finishing the
/// downstream session. Backend write, flush, or transform failures must prevent
/// a later finish from committing incomplete output. Rejected writes after
/// sealing do not alter already-staged output.
///
/// A successful finish or abort is terminal. Repeating the same operation is
/// harmless; conflicting operations and writes after sealing return errors.
/// Implementations retain in-flight operations across cancellation of their
/// futures. Cancellation cannot undo a commit already submitted to a backend.
/// Dropping a session never intentionally commits, but cannot await cleanup:
/// backend resources or staging data may remain. Await `abort` for cleanup.
#[async_trait(?Send)]
pub trait WriteFileSession: AsyncWrite + Unpin {
    /// Finalizes all output, then commits the downstream file.
    ///
    /// # Errors
    ///
    /// Returns write, transform, or commit failures, or an aborted-state error.
    async fn finish(&mut self) -> Result<(), WriteFileError>;

    /// Discards staged output and awaits backend cleanup.
    ///
    /// # Errors
    ///
    /// Returns cleanup failures or an error if commit has already started.
    async fn abort(&mut self) -> Result<(), WriteFileError>;
}

#[async_trait(?Send)]
impl<T: WriteFileSession + ?Sized> WriteFileSession for Box<T> {
    async fn finish(&mut self) -> Result<(), WriteFileError> {
        (**self).finish().await
    }

    async fn abort(&mut self) -> Result<(), WriteFileError> {
        (**self).abort().await
    }
}

/// A failure while writing, finishing, or aborting a file session.
#[derive(Debug, Clone, thiserror::Error)]
pub enum WriteFileError {
    #[error("storage operation is not supported")]
    NotSupported,
    #[error("storage entry not found")]
    NotFound,
    #[error("path is not a directory")]
    NotADirectory,
    #[error("path is not a file")]
    NotAFile,
    #[error("storage permission denied")]
    PermissionDenied,
    #[error("storage is disconnected")]
    Disconnected,
    #[error("storage is unavailable: {message}")]
    Unavailable { message: String },
    #[error("storage operation failed: {message}")]
    Failed { message: String },
}

impl From<WriteFileError> for std::io::Error {
    fn from(error: WriteFileError) -> Self {
        let kind = match error {
            WriteFileError::NotSupported => std::io::ErrorKind::Unsupported,
            WriteFileError::NotFound => std::io::ErrorKind::NotFound,
            WriteFileError::NotADirectory => std::io::ErrorKind::NotADirectory,
            WriteFileError::NotAFile => std::io::ErrorKind::IsADirectory,
            WriteFileError::PermissionDenied => {
                std::io::ErrorKind::PermissionDenied
            }
            WriteFileError::Disconnected => std::io::ErrorKind::NotConnected,
            _ => std::io::ErrorKind::Other,
        };
        Self::new(kind, error)
    }
}

impl From<std::io::Error> for WriteFileError {
    fn from(error: std::io::Error) -> Self {
        Self::from(&error)
    }
}

impl From<&std::io::Error> for WriteFileError {
    fn from(error: &std::io::Error) -> Self {
        if let Some(error) = error
            .get_ref()
            .and_then(|error| error.downcast_ref::<Self>())
        {
            return error.clone();
        }
        match error.kind() {
            std::io::ErrorKind::Unsupported => Self::NotSupported,
            std::io::ErrorKind::NotFound => Self::NotFound,
            std::io::ErrorKind::NotADirectory => Self::NotADirectory,
            std::io::ErrorKind::IsADirectory => Self::NotAFile,
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            std::io::ErrorKind::NotConnected => Self::Disconnected,
            _ => Self::Failed {
                message: error.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::domain::storage::{Storage, StoragePath};
    use crate::test_utils::mocks::{
        MemoryWriteStorage, TrailerWriteSession, WriteSessionState,
    };
    use futures::{FutureExt, io::AsyncWriteExt};
    use std::{cell::RefCell, rc::Rc};

    async fn start_session(
        storage: &dyn Storage,
    ) -> Box<dyn WriteFileSession + '_> {
        storage
            .start_write_file(StoragePath::root())
            .await
            .expect("start session")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn borrowed_non_send_sessions_write_incrementally_and_commit_explicitly()
     {
        let state = Rc::new(RefCell::new(WriteSessionState::default()));
        let storage: Box<dyn Storage + '_> =
            Box::new(MemoryWriteStorage(&state));
        let mut session = start_session(&storage).await;
        assert_eq!(session.write(b"abcdef").await.expect("partial write"), 2);
        session
            .write_all(b"cdef")
            .await
            .expect("write remaining input");
        session.flush().await.expect("flush");
        assert_eq!(state.borrow().staged, b"abcdef");
        assert!(state.borrow().committed.is_none());
        session.close().await.expect("seal input");
        assert!(session.write(b"late").await.is_err());
        assert!(state.borrow().committed.is_none());
        state.borrow_mut().delay_finish = true;
        assert!(session.finish().now_or_never().is_none());
        assert!(state.borrow().committed.is_none());
        state.borrow_mut().release();
        session.finish().await.expect("resume finish");
        session.finish().await.expect("repeat finish");
        assert_eq!(
            state.borrow().committed.as_deref(),
            Some(b"abcdef".as_slice())
        );
        assert_eq!(
            state
                .borrow()
                .events
                .iter()
                .filter(|event| **event == "finish")
                .count(),
            1
        );
        assert!(session.abort().await.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn abort_is_awaited_and_discards_staged_output() {
        let state = Rc::new(RefCell::new(WriteSessionState::default()));
        let storage = MemoryWriteStorage(&state);
        let mut session = start_session(&storage).await;
        session.write_all(b"discard").await.expect("write");
        state.borrow_mut().delay_abort = true;
        assert!(session.abort().now_or_never().is_none());
        assert!(!state.borrow().events.contains(&"abort"));
        assert!(session.finish().await.is_err());
        state.borrow_mut().release();
        session.abort().await.expect("resume abort");
        session.abort().await.expect("repeat abort");
        assert!(state.borrow().staged.is_empty());
        assert!(session.write(b"late").await.is_err());
        assert!(state.borrow().committed.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn drop_does_not_commit() {
        let state = Rc::new(RefCell::new(WriteSessionState::default()));
        let storage = MemoryWriteStorage(&state);
        let mut dropped = start_session(&storage).await;
        dropped.write_all(b"uncommitted").await.expect("write");
        drop(dropped);
        assert!(state.borrow().committed.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_commit_and_abort_failures_do_not_commit() {
        for failure in ["write", "finish", "abort"] {
            let state = Rc::new(RefCell::new(WriteSessionState::default()));
            let storage = MemoryWriteStorage(&state);
            let mut session = start_session(&storage).await;
            match failure {
                "write" => {
                    state.borrow_mut().fail_write = true;
                    assert!(session.write_all(b"data").await.is_err());
                }
                "finish" => {
                    session.write_all(b"data").await.expect("write");
                    state.borrow_mut().fail_finish = true;
                    assert!(session.finish().await.is_err());
                }
                _ => {
                    state.borrow_mut().fail_abort = true;
                    assert!(session.abort().await.is_err());
                }
            }
            assert!(session.finish().await.is_err());
            assert!(state.borrow().committed.is_none());
            state.borrow_mut().fail_abort = false;
            session.abort().await.expect("cleanup");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn transforms_finalize_before_downstream_commit_even_after_close() {
        let state = Rc::new(RefCell::new(WriteSessionState::default()));
        let storage = MemoryWriteStorage(&state);
        let inner = start_session(&storage).await;
        let mut session = TrailerWriteSession::new(inner);
        session.write_all(b"body").await.expect("write");
        session.close().await.expect("seal");
        assert_eq!(state.borrow().staged, b"body");
        assert!(state.borrow().committed.is_none());
        state.borrow_mut().delay_write = true;
        assert!(session.finish().now_or_never().is_none());
        assert!(!state.borrow().events.contains(&"finish"));
        state.borrow_mut().release();
        state.borrow_mut().delay_finish = true;
        assert!(session.finish().now_or_never().is_none());
        assert_eq!(state.borrow().staged, b"body<end>");
        state.borrow_mut().release();
        session.finish().await.expect("finish");
        session.finish().await.expect("repeat");
        assert_eq!(
            state.borrow().committed.as_deref(),
            Some(b"body<end>".as_slice())
        );
        assert_eq!(state.borrow().events.last(), Some(&"finish"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn nested_transforms_finish_in_order_and_propagate_downstream_failure()
     {
        for fail_downstream in [false, true] {
            let state = Rc::new(RefCell::new(WriteSessionState::default()));
            let storage = MemoryWriteStorage(&state);
            let inner = start_session(&storage).await;
            let mut session = TrailerWriteSession::new(Box::new(
                TrailerWriteSession::new(inner),
            ));
            session.write_all(b"body").await.expect("write");
            session.close().await.expect("seal outer input");
            state.borrow_mut().fail_finish = fail_downstream;
            let result = session.finish().await;
            assert_eq!(state.borrow().staged, b"body<end><end>");
            if fail_downstream {
                assert!(result.is_err());
                assert!(session.finish().await.is_err());
                assert!(state.borrow().committed.is_none());
                session.abort().await.expect("abort nested sessions");
            } else {
                result.expect("finish nested sessions");
                assert_eq!(
                    state.borrow().committed.as_deref(),
                    Some(b"body<end><end>".as_slice())
                );
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn transform_finalization_failure_aborts_without_downstream_finish() {
        let state = Rc::new(RefCell::new(WriteSessionState::default()));
        let storage = MemoryWriteStorage(&state);
        let inner = start_session(&storage).await;
        let mut session = TrailerWriteSession::new(inner);
        session.write_all(b"body").await.expect("write");
        session.fail_finalize = true;
        assert!(session.finish().await.is_err());
        assert!(session.finish().await.is_err());
        assert!(!state.borrow().events.contains(&"finish"));
        session.abort().await.expect("abort downstream");
        assert_eq!(state.borrow().events.last(), Some(&"abort"));
        assert!(state.borrow().committed.is_none());
    }

    #[test]
    fn write_errors_preserve_categories_across_async_io() {
        use std::io::{Error, ErrorKind};

        for (error, kind) in [
            (
                WriteFileError::PermissionDenied,
                ErrorKind::PermissionDenied,
            ),
            (WriteFileError::Disconnected, ErrorKind::NotConnected),
            (WriteFileError::NotADirectory, ErrorKind::NotADirectory),
            (WriteFileError::NotAFile, ErrorKind::IsADirectory),
        ] {
            let message = error.to_string();
            let io_error = Error::from(error);
            assert_eq!(io_error.kind(), kind);
            assert_eq!(WriteFileError::from(io_error).to_string(), message);
            assert_eq!(
                WriteFileError::from(Error::from(kind)).to_string(),
                message,
            );
        }
    }
}
