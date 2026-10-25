use super::{Error, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const DIR_CACHE_TTL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct CachedDirEntry {
    pub file_name: String,
    pub is_dir: bool,
    pub size: u64,
    pub accessed: SystemTime,
}

#[derive(Debug, Clone)]
pub struct CachedDir {
    pub entries: Vec<CachedDirEntry>,
    pub loaded_at: Instant,
}

#[derive(Debug)]
pub struct CachedDirCache {
    cache: RwLock<HashMap<PathBuf, CachedDir>>,
}

impl CachedDirCache {
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
        }
    }

    pub fn get(&self, path: &PathBuf) -> Option<Vec<CachedDirEntry>> {
        let guard = self.cache.read();
        let entry = guard.get(path)?;
        if entry.loaded_at.elapsed() > DIR_CACHE_TTL {
            return None;
        }
        Some(entry.entries.clone())
    }

    pub fn insert(&self, path: PathBuf, entries: Vec<CachedDirEntry>) {
        self.cache.write().insert(
            path,
            CachedDir {
                entries,
                loaded_at: Instant::now(),
            },
        );
    }

    pub fn invalidate(&self, path: &PathBuf) {
        self.cache.write().remove(path);
    }

    pub fn clear(&self) {
        self.cache.write().clear();
    }
}

impl Default for CachedDirCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg_attr(test, mockall::automock)]
pub trait CachePort: Send + Sync {
    fn sync_metadata_placeholders(
        &self,
        dir_path: &Path,
        entries: &[CachedDirEntry],
    ) -> Result<()>;
    fn read_range(&self, path: &Path, start: u64, end: u64) -> Result<Vec<u8>>;
    fn write_range(
        &self,
        path: &Path,
        start: u64,
        data: &[u8],
        size: u64,
    ) -> Result<()>;
}

pub struct NoCacheFsCache;

impl NoCacheFsCache {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoCacheFsCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CachePort for NoCacheFsCache {
    fn sync_metadata_placeholders(
        &self,
        _dir_path: &Path,
        _entries: &[CachedDirEntry],
    ) -> Result<()> {
        Ok(())
    }

    fn read_range(&self, path: &Path, start: u64, end: u64) -> Result<Vec<u8>> {
        Err(Error::CacheRangeNotCached {
            path: path.to_path_buf(),
            start,
            end,
        })
    }

    fn write_range(
        &self,
        _path: &Path,
        _start: u64,
        _data: &[u8],
        _size: u64,
    ) -> Result<()> {
        Ok(())
    }
}
