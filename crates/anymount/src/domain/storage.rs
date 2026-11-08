mod path;

pub use path::{InvalidStoragePath, StoragePath};

use std::{any::Any, future::Future, pin::Pin, time::SystemTime};

use async_trait::async_trait;

use super::started_authorization::{AuthStorageError, StartedAuthorization};

#[typetag::serde(tag = "type")]
pub trait StorageConfig: Send + Sync {
    fn connect(
        &self,
    ) -> StorageFuture<'_, Result<Box<dyn Storage>, ConnectStorageError>>;

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

/// A future that can run on single-threaded platforms.
pub type StorageFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Maximum number of bytes a single range read may request.
pub const MAX_READ_SIZE: usize = 65536;

/// Provides asynchronous access to a user-authorized storage workspace.
///
/// Range reads return at most the requested length and may return fewer bytes.
/// An empty result indicates the end of the file. Providers may report
/// unsupported operations and omit metadata they cannot determine.
#[cfg_attr(test, mockall::automock)]
#[async_trait(?Send)]
pub trait Storage: Any {
    async fn connect(
        &self,
        _request_permission: bool,
    ) -> Result<bool, StorageError> {
        Ok(true)
    }
    async fn disconnect(&self) -> Result<(), StorageError> {
        Ok(())
    }
    fn is_connected(&self) -> bool {
        true
    }
    async fn read_dir(
        &self,
        _path: StoragePath,
    ) -> Result<Vec<StorageEntry>, StorageError> {
        Err(StorageError::NotSupported)
    }
    async fn read_range(
        &self,
        _path: StoragePath,
        _offset: u64,
        _length: usize,
    ) -> Result<Vec<u8>, StorageError> {
        Err(StorageError::NotSupported)
    }
    async fn write(
        &self,
        _path: StoragePath,
        _bytes: &[u8],
    ) -> Result<(), StorageError> {
        Err(StorageError::NotSupported)
    }
    fn as_any(&self) -> &dyn Any;
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Describes a directory entry and its available metadata.
pub struct StorageEntry {
    pub name: String,
    pub is_directory: bool,
    pub size: Option<u64>,
    pub accessed: Option<SystemTime>,
}

#[derive(Debug, thiserror::Error)]
/// Reports a storage operation failure.
pub enum StorageError {
    #[error("storage operation is not supported")]
    NotSupported,
    #[error("storage entry not found")]
    NotFound,
    #[error("path is not a directory")]
    NotADirectory,
    #[error("path is not a file")]
    NotAFile,
    #[error("storage is unavailable: {message}")]
    Unavailable { message: String },
    #[error("storage is disconnected")]
    Disconnected,
    #[error("storage permission denied")]
    PermissionDenied,
    #[error("storage operation failed: {message}")]
    Failed { message: String },
}

#[async_trait(?Send)]
impl<T: Storage + ?Sized> Storage for Box<T> {
    async fn connect(&self, request: bool) -> Result<bool, StorageError> {
        (**self).connect(request).await
    }
    async fn disconnect(&self) -> Result<(), StorageError> {
        (**self).disconnect().await
    }
    fn is_connected(&self) -> bool {
        (**self).is_connected()
    }
    async fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<Vec<StorageEntry>, StorageError> {
        (**self).read_dir(path).await
    }
    async fn read_range(
        &self,
        path: StoragePath,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, StorageError> {
        (**self).read_range(path, offset, length).await
    }
    async fn write(
        &self,
        path: StoragePath,
        data: &[u8],
    ) -> Result<(), StorageError> {
        (**self).write(path, data).await
    }
    fn as_any(&self) -> &dyn Any {
        (**self).as_any()
    }
}

pub trait DirEntry: Send + Sync {
    // Should be &str
    fn file_name(&self) -> String;
    fn is_dir(&self) -> bool;
    fn size(&self) -> u64;
    fn accessed(&self) -> SystemTime;
}

pub trait WriteAt {
    fn write_at(&mut self, buf: &[u8], offset: u64)
    -> Result<(), WriteAtError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ReadDirError {
    #[error("storage does not support listing a directory")]
    NotSupported,

    #[error("directory not found")]
    NotFound,

    #[error("path is not a directory")]
    NotADirectory,

    #[error("permission denied")]
    PermissionDenied,

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
    #[error("storage does not support reading a file")]
    NotSupported,

    #[error("file not found")]
    NotFound,

    #[error("path is not a file")]
    NotAFile,

    #[error("permission denied")]
    PermissionDenied,

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
    impl Storage for TestStorageConnection {
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    // private test util: Tests storage defaults and deserialization.
    #[typetag::serde(name = "test")]
    impl StorageConfig for TestStorageConfig {
        fn clone_config(&self) -> Box<dyn StorageConfig> {
            Box::new(self.clone())
        }
        fn connect(
            &self,
        ) -> StorageFuture<'_, Result<Box<dyn Storage>, ConnectStorageError>>
        {
            Box::pin(async {
                Ok(Box::new(TestStorageConnection {}) as Box<dyn Storage>)
            })
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

        let connection = config.connect().await.expect("connect test storage");
        assert!(matches!(
            connection.read_dir(StoragePath::root()).await,
            Err(StorageError::NotSupported)
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
            Err(StorageError::NotSupported)
        ));
    }
}
