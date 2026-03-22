pub mod error;
pub mod local;
pub mod onedrive;
pub mod storage;

pub use error::{Error, Result};
pub use local::{LocalDirEntry, LocalStorage};
pub use onedrive::{OneDriveConfig, OneDriveDirEntry, OneDriveStorage};
pub use storage::{DirEntry, Storage, WriteAt};

pub use crate::domain::driver::StorageConfig as Config;

enum StorageVariant {
    Local(LocalStorage),
    OneDrive(OneDriveStorage),
}

pub enum StorageEntry {
    Local(LocalDirEntry),
    OneDrive(OneDriveDirEntry),
}

impl DirEntry for StorageEntry {
    fn file_name(&self) -> String {
        match self {
            Self::Local(e) => e.file_name(),
            Self::OneDrive(e) => e.file_name(),
        }
    }
    fn is_dir(&self) -> bool {
        match self {
            Self::Local(e) => e.is_dir(),
            Self::OneDrive(e) => e.is_dir(),
        }
    }
    fn size(&self) -> u64 {
        match self {
            Self::Local(e) => e.size(),
            Self::OneDrive(e) => e.size(),
        }
    }
    fn accessed(&self) -> std::time::SystemTime {
        match self {
            Self::Local(e) => e.accessed(),
            Self::OneDrive(e) => e.accessed(),
        }
    }
}

pub struct StorageIter {
    local_iter: Option<std::vec::IntoIter<StorageEntry>>,
    onedrive_iter: Option<std::vec::IntoIter<StorageEntry>>,
}

impl Iterator for StorageIter {
    type Item = StorageEntry;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(ref mut iter) = self.local_iter {
            iter.next()
        } else if let Some(ref mut iter) = self.onedrive_iter {
            iter.next()
        } else {
            None
        }
    }
}

pub struct ConcreteStorage {
    variant: StorageVariant,
}

impl Storage for ConcreteStorage {
    type Entry = StorageEntry;
    type Iter = StorageIter;

    fn read_dir(&self, path: std::path::PathBuf) -> Result<Self::Iter> {
        match &self.variant {
            StorageVariant::Local(s) => {
                let entries: Vec<_> = s.read_dir(path)?.collect();
                let storage_entries: Vec<_> =
                    entries.into_iter().map(StorageEntry::Local).collect();
                Ok(StorageIter {
                    local_iter: Some(storage_entries.into_iter()),
                    onedrive_iter: None,
                })
            }
            StorageVariant::OneDrive(s) => {
                let entries: Vec<_> = s.read_dir(path)?.collect();
                let storage_entries: Vec<_> =
                    entries.into_iter().map(StorageEntry::OneDrive).collect();
                Ok(StorageIter {
                    local_iter: None,
                    onedrive_iter: Some(storage_entries.into_iter()),
                })
            }
        }
    }

    fn read_file_at(
        &self,
        path: std::path::PathBuf,
        writer: &mut impl WriteAt,
        range: std::ops::Range<u64>,
    ) -> Result<()> {
        match &self.variant {
            StorageVariant::Local(s) => s.read_file_at(path, writer, range),
            StorageVariant::OneDrive(s) => s.read_file_at(path, writer, range),
        }
    }
}

pub fn new(config: Config) -> Result<impl Storage> {
    match config {
        Config::Local { root } => Ok(ConcreteStorage {
            variant: StorageVariant::Local(LocalStorage::new(root)),
        }),
        Config::OneDrive {
            root,
            endpoint,
            access_token,
            refresh_token,
            client_id,
            token_expiry_buffer_secs,
        } => {
            let config = OneDriveConfig {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            };
            Ok(ConcreteStorage {
                variant: StorageVariant::OneDrive(config.connect()?),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn new_local_storage() {
        let dir = TempDir::new().unwrap();
        let config = Config::Local {
            root: dir.path().to_path_buf(),
        };
        let storage = new(config).unwrap();
        let entries: Vec<_> = storage
            .read_dir(std::path::PathBuf::new())
            .unwrap()
            .collect();
        assert!(entries.is_empty());
    }

    #[test]
    fn new_onedrive_storage_with_refresh_token() {
        let config = Config::OneDrive {
            root: std::path::PathBuf::from("/"),
            endpoint: "https://graph.microsoft.com/v1.0".into(),
            access_token: None,
            refresh_token: Some("test_refresh_token".into()),
            client_id: None,
            token_expiry_buffer_secs: None,
        };
        let result = new(config);
        assert!(result.is_ok());
    }

    #[test]
    fn new_onedrive_storage_fails_without_token() {
        let config = Config::OneDrive {
            root: std::path::PathBuf::from("/"),
            endpoint: "https://graph.microsoft.com/v1.0".into(),
            access_token: None,
            refresh_token: None,
            client_id: None,
            token_expiry_buffer_secs: None,
        };
        let result = new(config);
        assert!(result.is_err());
    }
}
