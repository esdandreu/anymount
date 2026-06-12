use super::{Error, Result};
use std::{ops::Range, path::PathBuf, time::SystemTime};

pub trait DirEntry: Send + Sync {
    // Should be &str
    fn file_name(&self) -> String;
    fn is_dir(&self) -> bool;
    fn size(&self) -> u64;
    fn accessed(&self) -> SystemTime;
}

pub trait WriteAt {
    fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<()>;
}

pub trait Storage {
    fn read_dir(
        &self,
        _path: PathBuf,
    ) -> Result<Box<dyn Iterator<Item = Box<dyn DirEntry>>>> {
        Err(Error::NotSupported {
            operation: "read_dir",
        })
    }

    fn read_file_at(
        &self,
        _path: PathBuf,
        _writer: &mut dyn WriteAt,
        _range: Range<u64>,
    ) -> Result<()> {
        Err(Error::NotSupported {
            operation: "read_file_at",
        })
    }
}

#[typetag::serde(tag = "type")]
pub trait StorageConfig {
    fn connect(&self) -> Result<Box<dyn Storage>>;

    fn kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

#[cfg(test)]
mod test {
    use super::{Error, Result, Storage, StorageConfig};
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
        fn connect(&self) -> Result<Box<dyn Storage>> {
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
            Err(Error::NotSupported {
                operation: "read_dir"
            })
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
            Err(Error::NotSupported {
                operation: "read_dir"
            })
        ));
    }
}
