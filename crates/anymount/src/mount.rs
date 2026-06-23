use std::path::Path;

use crate::storages::storage::StorageConfig;

pub trait Mount {
    fn path(&self) -> &Path;
    fn kind(&self) -> &'static str;
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct MountConfig {
    // path: Path, ?
    storage: Box<dyn StorageConfig>,
    // driver: Option<Box<dyn DriverConfig>>,
}

#[cfg(test)]
mod test {
    use super::MountConfig;
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::storages::{DirEntry, Result, Storage, StorageConfig};

    #[typetag::serde(name = "mount-test")]
    impl StorageConfig for TestStorageConfig {
        fn connect(&self) -> Result<Box<dyn Storage>> {
            Ok(Box::new(TestStorageConnection {
                config: self.clone(),
            }))
        }
    }

    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    struct TestStorageConfig {
        file_name: String,
    }

    struct TestStorageConnection {
        config: TestStorageConfig,
    }

    impl Storage for TestStorageConnection {
        fn read_dir(
            &self,
            _path: PathBuf,
        ) -> Result<Box<dyn Iterator<Item = Box<dyn DirEntry>>>> {
            let entry: Box<dyn DirEntry> = Box::new(TestDirEntry {
                file_name: self.config.file_name.clone(),
            });
            Ok(Box::new(std::iter::once(entry)))
        }
    }

    struct TestDirEntry {
        file_name: String,
    }

    impl DirEntry for TestDirEntry {
        fn file_name(&self) -> String {
            self.file_name.clone()
        }

        fn is_dir(&self) -> bool {
            false
        }

        fn size(&self) -> u64 {
            15
        }

        fn accessed(&self) -> SystemTime {
            UNIX_EPOCH
        }
    }

    #[test]
    fn deserializes_mount_config_from_toml() {
        let config: MountConfig = toml::from_str(
            r#"
    [storage]
    type = "mount-test"
    file_name = "hello-world.txt"
    "#,
        )
        .expect("deserialize mount config");

        let connection =
            config.storage.connect().expect("connect test storage");
        let mut entries = connection
            .read_dir(PathBuf::from("/"))
            .expect("read test storage root");

        let entry = entries.next().expect("get first entry");

        assert_eq!(entry.file_name(), "hello-world.txt");
        assert_eq!(entries.count(), 0);
    }
}
