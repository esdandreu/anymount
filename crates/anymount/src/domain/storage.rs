use std::{ops::Range, path::PathBuf, time::SystemTime};

#[typetag::serde(tag = "type")]
pub trait StorageConfig {
    fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError>;

    fn kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("failed to connect {kind} storage: {message}")]
pub struct ConnectStorageError {
    pub kind: &'static str,
    pub message: String,
}

pub trait Storage: Send + Sync {
    fn read_dir(
        &self,
        _path: PathBuf,
    ) -> Result<Box<dyn Iterator<Item = Box<dyn DirEntry>>>, ReadDirError> {
        Err(ReadDirError::NotSupported)
    }

    fn read_file_at(
        &self,
        _path: PathBuf,
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
        path: PathBuf,
    ) -> Result<Box<dyn Iterator<Item = Box<dyn DirEntry>>>, ReadDirError> {
        (**self).read_dir(path)
    }

    fn read_file_at(
        &self,
        path: PathBuf,
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

    // TODO remove, use one of the others
    #[error("storage returned an invalid response: {message}")]
    InvalidResponse { message: String },

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

    #[error("storage returned an invalid response: {message}")]
    InvalidResponse { message: String },

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
    use std::path::PathBuf;

    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    struct TestStorageConfig {
        endpoint: Option<String>,
        bucket: String,
        username: String,
        password: String,
    }

    struct TestStorageConnection {}

    impl Storage for TestStorageConnection {}

    #[typetag::serde(name = "test")]
    impl StorageConfig for TestStorageConfig {
        fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
            Ok(Box::new(TestStorageConnection {}))
        }
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
            connection.read_dir(PathBuf::from("/")),
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
            connection.read_dir("/".into()),
            Err(ReadDirError::NotSupported)
        ));
    }
}
