mod path;
mod write_file_session;

pub use path::{InvalidStoragePath, StoragePath};
#[doc(inline)]
pub use write_file_session::{WriteFileError, WriteFileSession};

use std::{ops::Range, pin::Pin, sync::Arc, time::SystemTime};

use async_trait::async_trait;
use futures::{Stream, io::AsyncWrite};

use super::{Authorization, AuthorizationError};
use crate::drivers::oauth::RuntimeBounds;

#[typetag::serde(tag = "type")]
pub trait StorageConfig: dyn_clone::DynClone + Send + Sync {
    /// Creates a connector with independent live state for a saved record.
    ///
    /// Use the same host-selected `identity` when resuming authorization.
    /// Clone the handle rather than creating another connector to share state.
    ///
    /// # Errors
    ///
    /// Returns a setup error when the provider cannot use this configuration.
    fn create_connector(
        &self,
        identity: String,
    ) -> Result<Arc<dyn StorageConnector>, AuthorizationError>;

    fn kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// Creates storage handles that share one record's live provider state.
#[async_trait(?Send)]
pub trait StorageConnector: RuntimeBounds {
    /// Exposes sign-in before a connection exists, if the provider supports it.
    fn authorization(&self) -> Option<&dyn Authorization> {
        None
    }

    /// Connects without starting sign-in or saving configuration.
    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError>;
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
    /// Copies a source range sequentially into the sink's current position.
    ///
    /// Returns the number of bytes copied, which may be shorter at EOF.
    /// Source offsets do not reposition the sink. Partial writes and
    /// backpressure are awaited. The sink and future need not be `Send`.
    /// This operation never flushes, closes, finishes, or aborts the sink.
    ///
    /// # Errors
    ///
    /// Returns source access or range errors, or [`ReadFileAtError::Write`]
    /// if the sink fails or cannot make progress.
    async fn read_file_at(
        &self,
        _path: StoragePath,
        _writer: &mut (dyn AsyncWrite + Unpin),
        _range: std::ops::Range<u64>,
    ) -> Result<u64, ReadFileAtError> {
        Err(ReadFileAtError::NotSupported)
    }
    /// Starts a streaming replacement for a file.
    ///
    /// The session may borrow this storage and need not be `Send`. Writes and
    /// flushes do not commit; callers must finish or explicitly abort it.
    ///
    /// # Errors
    ///
    /// Returns an error if writing is unsupported or the session cannot start.
    async fn start_write_file(
        &self,
        _path: StoragePath,
    ) -> Result<Box<dyn WriteFileSession + '_>, StartWriteFileError> {
        Err(StartWriteFileError::NotSupported)
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
        writer: &mut (dyn AsyncWrite + Unpin),
        range: Range<u64>,
    ) -> Result<u64, ReadFileAtError> {
        (**self).read_file_at(path, writer, range).await
    }
    async fn start_write_file(
        &self,
        path: StoragePath,
    ) -> Result<Box<dyn WriteFileSession + '_>, StartWriteFileError> {
        (**self).start_write_file(path).await
    }
}

pub trait DirEntry: Send + Sync {
    fn file_name(&self) -> String;
    fn is_dir(&self) -> bool;
    fn size(&self) -> Option<u64>;
    fn accessed(&self) -> Option<SystemTime>;
}

#[derive(Debug, thiserror::Error)]
pub enum ReadDirError {
    #[error(transparent)]
    Authorization(#[from] AuthorizationError),
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
pub enum ReadFileAtError {
    #[error(transparent)]
    Authorization(#[from] AuthorizationError),
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
    Write(#[source] std::io::Error),

    #[error("failed to read file: {message}")]
    Unknown { message: String },
}

/// A failure to start a file write session.
#[derive(Debug, thiserror::Error)]
pub enum StartWriteFileError {
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
    use crate::test_utils::mocks::NoOpStorage;

    #[tokio::test(flavor = "current_thread")]
    async fn writes_are_unsupported_by_default() {
        let storage: Box<dyn Storage> = Box::new(NoOpStorage);
        assert!(matches!(
            storage.start_write_file(StoragePath::root()).await,
            Err(StartWriteFileError::NotSupported)
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
        let session = cloned
            .create_connector("fixture".to_owned())
            .expect("open session");
        let connection = session.connect().await.expect("connect test storage");
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

        let session = config
            .create_connector("fixture".to_owned())
            .expect("open session");
        let connection = session.connect().await.expect("connect test storage");
        assert!(matches!(
            connection.read_dir(StoragePath::root()).await,
            Err(ReadDirError::NotSupported)
        ));
    }
}
