mod path;

pub use path::{InvalidStoragePath, StoragePath};

use std::{ops::Range, pin::Pin, time::SystemTime};

use futures::{Stream, io::AsyncWrite};

use async_trait::async_trait;

use super::started_authorization::{AuthStorageError, StartedAuthorization};

#[typetag::serde(tag = "type")]
#[async_trait(?Send)]
pub trait StorageConfig: dyn_clone::DynClone + Send + Sync {
    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError>;

    fn kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Starts authorization for this storage configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization cannot be started.
    fn start_authorization(
        self: Box<Self>,
    ) -> Result<Box<dyn StartedAuthorization>, AuthStorageError> {
        Err(AuthStorageError::NotImplemented { kind: self.kind() })
    }
}

dyn_clone::clone_trait_object!(StorageConfig);

#[derive(Debug, thiserror::Error)]
pub enum ConnectStorageError {
    #[error("{kind} storage is not authenticated")]
    NotAuthenticated { kind: &'static str },

    #[error("failed to connect {kind} storage: {message}")]
    Failed { kind: &'static str, message: String },
}

/// Maximum number of bytes a single range read may request.
pub const MAX_READ_FILE_AT_RANGE_SIZE: usize = 65536;

pub type DirStream =
    Pin<Box<dyn Stream<Item = Result<Box<dyn DirEntry>, ReadDirError>>>>;

#[async_trait(?Send)]
pub trait Storage {
    async fn read_dir(
        &self,
        _path: StoragePath,
    ) -> Result<DirStream, ReadDirError> {
        Err(ReadDirError::NotSupported)
    }
    async fn read_file_at(
        &self,
        _path: StoragePath,
        _writer: &mut dyn WriteAt,
        _range: std::ops::Range<u64>,
    ) -> Result<u64, ReadFileAtError> {
        Err(ReadFileAtError::NotSupported)
    }
    /// Opens a streaming replacement for a file.
    ///
    /// The session may borrow this storage and need not be `Send`. Writes and
    /// flushes do not commit; callers must finish or explicitly abort it.
    ///
    /// # Errors
    ///
    /// Returns an error if writing is unsupported or the file cannot be opened.
    async fn open_write(
        &self,
        _path: StoragePath,
    ) -> Result<Box<dyn WriteSession + '_>, WriteFileError> {
        Err(WriteFileError::NotSupported)
    }
}

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
pub trait WriteSession: AsyncWrite + Unpin {
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
impl<T: WriteSession + ?Sized> WriteSession for Box<T> {
    async fn finish(&mut self) -> Result<(), WriteFileError> {
        (**self).finish().await
    }

    async fn abort(&mut self) -> Result<(), WriteFileError> {
        (**self).abort().await
    }
}

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

#[async_trait(?Send)]
impl<T: Storage + ?Sized> Storage for Box<T> {
    async fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<DirStream, ReadDirError> {
        (**self).read_dir(path).await
    }
    async fn read_file_at(
        &self,
        path: StoragePath,
        writer: &mut dyn WriteAt,
        range: Range<u64>,
    ) -> Result<u64, ReadFileAtError> {
        (**self).read_file_at(path, writer, range).await
    }
    async fn open_write(
        &self,
        path: StoragePath,
    ) -> Result<Box<dyn WriteSession + '_>, WriteFileError> {
        (**self).open_write(path).await
    }
}

pub trait DirEntry: Send + Sync {
    // Should be &str
    fn file_name(&self) -> String;
    fn is_dir(&self) -> bool;
    fn size(&self) -> Option<u64>;
    fn accessed(&self) -> Option<SystemTime>;
}

pub trait WriteAt {
    fn write_at(&mut self, buf: &[u8], offset: u64)
    -> Result<(), WriteAtError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ReadDirError {
    #[error("invalid storage path")]
    InvalidPath(#[from] InvalidStoragePath),

    #[error("storage does not support listing a directory")]
    NotSupported,

    #[error("directory not found")]
    NotFound,

    #[error("path is not a directory")]
    NotADirectory,

    #[error("permission denied")]
    PermissionDenied,

    #[error("storage is disconnected")]
    Disconnected,

    #[error("storage is unavailable: {message}")]
    Unavailable { message: String },

    #[error("failed to list directory: {message}")]
    Unknown { message: String },
}

impl From<std::io::Error> for ReadDirError {
    fn from(source: std::io::Error) -> Self {
        match source.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound,
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            std::io::ErrorKind::NotADirectory => Self::NotADirectory,
            _ => Self::Unknown {
                message: source.to_string(),
            },
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WriteAtError {
    #[error("failed to write at offset {offset}: {message}")]
    Failed { offset: u64, message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ReadFileAtError {
    #[error("invalid storage path")]
    InvalidPath(#[from] InvalidStoragePath),

    #[error("storage does not support reading a file")]
    NotSupported,

    #[error("file not found")]
    NotFound,

    #[error("path is not a file")]
    NotAFile,

    #[error("permission denied")]
    PermissionDenied,

    #[error("storage is disconnected")]
    Disconnected,

    #[error("range extends beyond end of file")]
    RangeNotSatisfiable,

    #[error("storage is unavailable: {message}")]
    Unavailable { message: String },

    #[error("failed to write content")]
    WriteAt(#[from] WriteAtError),

    #[error("failed to read file: {message}")]
    Unknown { message: String },
}

impl From<std::io::Error> for ReadFileAtError {
    fn from(source: std::io::Error) -> Self {
        match source.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound,
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            std::io::ErrorKind::IsADirectory => Self::NotAFile,
            std::io::ErrorKind::UnexpectedEof => Self::RangeNotSatisfiable,
            _ => Self::Unknown {
                message: source.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::mocks::{
        MemoryWriteStorage, NoOpStorage, TrailerWriteSession, WriteSessionState,
    };
    use futures::{FutureExt, io::AsyncWriteExt};
    use std::{cell::RefCell, rc::Rc};

    #[tokio::test(flavor = "current_thread")]
    async fn writes_are_unsupported_by_default() {
        let storage: Box<dyn Storage> = Box::new(NoOpStorage);
        assert!(matches!(
            storage.open_write(StoragePath::root()).await,
            Err(WriteFileError::NotSupported)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn borrowed_non_send_sessions_write_incrementally_and_commit_explicitly()
     {
        let state = Rc::new(RefCell::new(WriteSessionState::default()));
        let storage: Box<dyn Storage + '_> =
            Box::new(MemoryWriteStorage(&state));
        let mut session =
            storage.open_write(StoragePath::root()).await.expect("open");
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
    async fn abort_is_awaited_and_drop_does_not_commit() {
        let state = Rc::new(RefCell::new(WriteSessionState::default()));
        let storage = MemoryWriteStorage(&state);
        let mut session =
            storage.open_write(StoragePath::root()).await.expect("open");
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

        let mut dropped =
            storage.open_write(StoragePath::root()).await.expect("open");
        dropped.write_all(b"uncommitted").await.expect("write");
        drop(dropped);
        assert!(state.borrow().committed.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_commit_and_abort_failures_do_not_commit() {
        for failure in ["write", "finish", "abort"] {
            let state = Rc::new(RefCell::new(WriteSessionState::default()));
            let storage = MemoryWriteStorage(&state);
            let mut session =
                storage.open_write(StoragePath::root()).await.expect("open");
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
        let inner =
            storage.open_write(StoragePath::root()).await.expect("open");
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
            let inner =
                storage.open_write(StoragePath::root()).await.expect("open");
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
        let inner =
            storage.open_write(StoragePath::root()).await.expect("open");
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

    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    struct TestStorageConfig {
        endpoint: Option<String>,
        bucket: String,
        username: String,
        password: String,
    }

    struct TestStorageConnection {}

    // private test util: Tests the storage configuration contract.
    #[async_trait(?Send)]
    impl Storage for TestStorageConnection {}

    // private test util: Tests storage defaults and deserialization.
    #[typetag::serde(name = "test")]
    #[async_trait(?Send)]
    impl StorageConfig for TestStorageConfig {
        async fn connect(
            &self,
        ) -> Result<Box<dyn Storage>, ConnectStorageError> {
            Ok(Box::new(TestStorageConnection {}))
        }
    }

    #[test]
    fn authorization_is_not_implemented_by_default() {
        let config = Box::new(TestStorageConfig {
            endpoint: None,
            bucket: "documents".to_owned(),
            username: "user".to_owned(),
            password: "secret".to_owned(),
        });

        assert!(matches!(
            config.start_authorization(),
            Err(AuthStorageError::NotImplemented { .. })
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn deserializes_test_storage_config_from_toml() {
        let config: Box<dyn StorageConfig> = toml::from_str(
            r#"
type = "test"
endpoint = "https://storage.example.test"
bucket = "documents"
username = "user"
password = "secret"
"#,
        )
        .expect("deserialize storage config");

        let serialized =
            toml::to_string(&config).expect("serialize storage config");
        let restored: Box<dyn StorageConfig> = toml::from_str(&serialized)
            .expect("deserialize serialized storage config");
        let cloned = restored.clone();
        drop(restored);
        assert_eq!(
            toml::to_string(&cloned).expect("serialize cloned storage config"),
            serialized
        );
        let connection = cloned.connect().await.expect("connect test storage");
        assert!(matches!(
            connection.read_dir(StoragePath::root()).await,
            Err(ReadDirError::NotSupported)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn deserializes_test_storage_config_without_endpoint() {
        let config: Box<dyn StorageConfig> = toml::from_str(
            r#"
type = "test"
bucket = "documents"
username = "user"
password = "secret"
"#,
        )
        .expect("deserialize storage config");

        let connection = config.connect().await.expect("connect test storage");
        assert!(matches!(
            connection.read_dir(StoragePath::root()).await,
            Err(ReadDirError::NotSupported)
        ));
    }
}
