mod path;

pub use path::{InvalidStoragePath, StoragePath};

use std::{ops::Range, pin::Pin, time::SystemTime};

use futures::Stream;

use async_trait::async_trait;

use super::started_authorization::{AuthStorageError, StartedAuthorization};

#[typetag::serde(tag = "type")]
#[async_trait(?Send)]
pub trait StorageConfig: Send + Sync {
    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError>;

    fn clone_config(&self) -> Box<dyn StorageConfig>;

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
#[cfg_attr(test, mockall::automock)]
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
    async fn write_file(
        &self,
        _path: StoragePath,
        _bytes: &[u8],
    ) -> Result<(), WriteFileError> {
        Err(WriteFileError::NotSupported)
    }
}

#[derive(Debug, thiserror::Error)]
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
    async fn write_file(
        &self,
        path: StoragePath,
        data: &[u8],
    ) -> Result<(), WriteFileError> {
        (**self).write_file(path, data).await
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
        fn clone_config(&self) -> Box<dyn StorageConfig> {
            Box::new(self.clone())
        }
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
        let cloned = restored.clone_config();
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
