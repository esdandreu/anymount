mod path;

pub use path::{InvalidStoragePath, StoragePath};

use std::{any::Any, future::Future, ops::Range, pin::Pin, time::SystemTime};

use async_trait::async_trait;

use super::started_authorization::{AuthStorageError, StartedAuthorization};

#[typetag::serde(tag = "type")]
pub trait StorageConfig: Send + Sync {
    fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError>;

    /// Creates an asynchronous storage provider.
    ///
    /// Connect the returned provider before reading or writing files.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider cannot be opened.
    fn connect_async(
        &self,
    ) -> StorageFuture<'_, Result<Box<dyn AsyncStorage>, ConnectStorageError>>
    {
        Box::pin(async {
            Err(ConnectStorageError::Failed {
                kind: self.kind(),
                message: "asynchronous storage is not supported".to_owned(),
            })
        })
    }

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

/// A storage provider with asynchronous byte operations.
///
/// Call `connect` before I/O. Requesting permission may require a user gesture.
#[async_trait(?Send)]
pub trait AsyncStorage: Any {
    /// Queries access and optionally requests permission from the user.
    ///
    /// # Errors
    ///
    /// Returns an error if the permission operation fails.
    async fn connect(
        &self,
        request_permission: bool,
    ) -> Result<bool, AsyncStorageError>;
    /// Disconnects this connection without deleting its data.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider cannot disconnect.
    async fn disconnect(&self) -> Result<(), AsyncStorageError>;
    /// Reports the last known connection state.
    fn is_connected(&self) -> bool;
    /// Lists the entries in a directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be listed.
    async fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<Vec<StorageEntry>, AsyncStorageError>;
    /// Reads the entire file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    async fn read(
        &self,
        path: StoragePath,
    ) -> Result<Vec<u8>, AsyncStorageError>;
    /// Replaces the entire file with the supplied bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    async fn write(
        &self,
        path: StoragePath,
        bytes: &[u8],
    ) -> Result<(), AsyncStorageError>;
    /// Exposes the provider for platform-specific handle interoperation.
    fn as_any(&self) -> &dyn Any;
}

/// A file or directory within a storage directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageEntry {
    /// The entry's name within its parent directory.
    pub name: String,
    /// Whether the entry is a directory.
    pub is_directory: bool,
}

/// An asynchronous storage operation failed.
#[derive(Debug, thiserror::Error)]
pub enum AsyncStorageError {
    #[error("storage entry not found")]
    NotFound,
    #[error("storage is disconnected")]
    Disconnected,
    #[error("storage permission denied")]
    PermissionDenied,
    #[error("storage operation failed: {message}")]
    Failed { message: String },
}

#[cfg_attr(test, mockall::automock)]
pub trait Storage: Send + Sync {
    fn read_dir(
        &self,
        _path: StoragePath,
    ) -> Result<Box<dyn Iterator<Item = Box<dyn DirEntry>>>, ReadDirError> {
        Err(ReadDirError::NotSupported)
    }

    fn read_file_at(
        &self,
        _path: StoragePath,
        _writer: &mut dyn WriteAt,
        _range: Range<u64>,
    ) -> Result<(), ReadFileAtError> {
        Err(ReadFileAtError::NotSupported)
    }
}

impl<T> Storage for Box<T>
where
    T: Storage + ?Sized,
{
    fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<Box<dyn Iterator<Item = Box<dyn DirEntry>>>, ReadDirError> {
        (**self).read_dir(path)
    }

    fn read_file_at(
        &self,
        path: StoragePath,
        writer: &mut dyn WriteAt,
        range: Range<u64>,
    ) -> Result<(), ReadFileAtError> {
        (**self).read_file_at(path, writer, range)
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
    impl Storage for TestStorageConnection {}

    // private test util: Tests storage defaults and deserialization.
    #[typetag::serde(name = "test")]
    impl StorageConfig for TestStorageConfig {
        fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
            Ok(Box::new(TestStorageConnection {}))
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn async_connection_is_unsupported_by_default() {
        let config: Box<dyn StorageConfig> = Box::new(TestStorageConfig {
            endpoint: None,
            bucket: "documents".to_owned(),
            username: "user".to_owned(),
            password: "secret".to_owned(),
        });

        assert!(matches!(
            config.connect_async().await,
            Err(ConnectStorageError::Failed { .. })
        ));
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

    #[test]
    fn deserializes_test_storage_config_from_toml() {
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

        let connection = config.connect().expect("connect test storage");
        assert!(matches!(
            connection.read_dir(StoragePath::root()),
            Err(ReadDirError::NotSupported)
        ));
    }

    #[test]
    fn deserializes_test_storage_config_without_endpoint() {
        let config: Box<dyn StorageConfig> = toml::from_str(
            r#"
type = "test"
bucket = "documents"
username = "user"
password = "secret"
"#,
        )
        .expect("deserialize storage config");

        let connection = config.connect().expect("connect test storage");
        assert!(matches!(
            connection.read_dir(StoragePath::root()),
            Err(ReadDirError::NotSupported)
        ));
    }
}
