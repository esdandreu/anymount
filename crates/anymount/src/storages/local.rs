use std::path::PathBuf;
use std::time::SystemTime;

use super::storage::{DirEntry, Storage};

pub struct LocalStorage {
    root: PathBuf,
}

pub struct LocalDirEntry {
    file_name: String,
    is_dir: bool,
    size: u64,
    accessed: SystemTime,
}

impl LocalStorage {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl DirEntry for LocalDirEntry {
    fn file_name(&self) -> String {
        self.file_name.clone()
    }
    fn is_dir(&self) -> bool {
        self.is_dir
    }
    fn size(&self) -> u64 {
        self.size
    }
    fn accessed(&self) -> SystemTime {
        self.accessed
    }
}

impl Storage for LocalStorage {
    type Entry = LocalDirEntry;
    type Iter = std::vec::IntoIter<LocalDirEntry>;

    fn read_dir(&self, path: PathBuf) -> std::result::Result<Self::Iter, String> {
        let full_path = self.root.join(path);
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&full_path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let meta = entry.metadata().map_err(|e| e.to_string())?;
            let accessed = meta
                .accessed()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            entries.push(LocalDirEntry {
                file_name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                size: meta.len(),
                accessed,
            });
        }
        Ok(entries.into_iter())
    }
}
