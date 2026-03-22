// Copyright 2026 Dotphoton AG

//! Linux FUSE filesystem with sparse file caching backed by the [`Storage`] trait.

use super::{Error, Result};
use crate::Logger;
use crate::drivers::fuse::{
    CachePort as FuseCachePort, CachedDirEntry, StorageFilesystem as FuseStorageFilesystem,
};
use crate::storages::{DirEntry, Storage, WriteAt};
use std::collections::{BTreeSet, HashMap};
use std::fs::OpenOptions;
use std::os::unix::fs::FileExt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

const DATA_CACHE_BLOCK_SIZE: u64 = 64 * 1024;

#[derive(Debug)]
pub struct SparseFsCache {
    cache_root: PathBuf,
    data_cache_blocks: parking_lot::RwLock<HashMap<PathBuf, BTreeSet<u64>>>,
}

impl SparseFsCache {
    pub fn new(cache_root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&cache_root).map_err(|source| Error::CacheIo {
            operation: "create cache root",
            path: cache_root.clone(),
            source,
        })?;
        Ok(Self {
            cache_root,
            data_cache_blocks: parking_lot::RwLock::new(HashMap::new()),
        })
    }

    fn cache_relative_path(path: &Path) -> PathBuf {
        path.components()
            .filter_map(|component| match component {
                Component::Normal(part) => Some(part.to_owned()),
                _ => None,
            })
            .collect()
    }

    fn cache_path(&self, path: &Path) -> PathBuf {
        self.cache_root.join(Self::cache_relative_path(path))
    }

    fn ensure_sparse_file(path: &Path, size: u64) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::CacheIo {
                operation: "create cache parent",
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .open(path)
            .map_err(|source| Error::CacheIo {
                operation: "open cache file",
                path: path.to_path_buf(),
                source,
            })?;
        file.set_len(size).map_err(|source| Error::CacheIo {
            operation: "resize cache file",
            path: path.to_path_buf(),
            source,
        })?;
        Ok(())
    }

    fn ensure_placeholder(&self, relative_path: &Path, is_dir: bool, size: u64) -> Result<()> {
        let metadata_path = self.cache_path(relative_path);
        if is_dir {
            std::fs::create_dir_all(&metadata_path).map_err(|source| Error::CacheIo {
                operation: "create metadata directory",
                path: metadata_path.clone(),
                source,
            })?;
        } else {
            Self::ensure_sparse_file(&metadata_path, size)?;
        }
        Ok(())
    }

    fn cache_blocks_for_range(start: u64, end: u64) -> impl Iterator<Item = u64> {
        let first = start / DATA_CACHE_BLOCK_SIZE;
        let last = (end.saturating_sub(1)) / DATA_CACHE_BLOCK_SIZE;
        first..=last
    }

    fn is_range_cached(&self, path: &Path, start: u64, end: u64) -> bool {
        if end <= start {
            return true;
        }
        let blocks = self.data_cache_blocks.read();
        let Some(cached_blocks) = blocks.get(path) else {
            return false;
        };
        Self::cache_blocks_for_range(start, end).all(|block| cached_blocks.contains(&block))
    }

    fn mark_range_cached(&self, path: &Path, start: u64, end: u64) {
        if end <= start {
            return;
        }
        let mut blocks = self.data_cache_blocks.write();
        let cached_blocks = blocks.entry(path.to_path_buf()).or_default();
        for block in Self::cache_blocks_for_range(start, end) {
            cached_blocks.insert(block);
        }
    }

    fn read_exact_at(
        path: &Path,
        file: &std::fs::File,
        mut buf: &mut [u8],
        mut offset: u64,
    ) -> Result<()> {
        while !buf.is_empty() {
            let read = file.read_at(buf, offset).map_err(|source| Error::CacheIo {
                operation: "read cache file",
                path: path.to_path_buf(),
                source,
            })?;
            if read == 0 {
                return Err(Error::CacheUnexpectedEof {
                    path: path.to_path_buf(),
                });
            }
            offset += read as u64;
            buf = &mut buf[read..];
        }
        Ok(())
    }

    fn write_all_at(
        path: &Path,
        file: &std::fs::File,
        mut buf: &[u8],
        mut offset: u64,
    ) -> Result<()> {
        while !buf.is_empty() {
            let written = file
                .write_at(buf, offset)
                .map_err(|source| Error::CacheIo {
                    operation: "write cache file",
                    path: path.to_path_buf(),
                    source,
                })?;
            if written == 0 {
                return Err(Error::CacheIo {
                    operation: "write cache file",
                    path: path.to_path_buf(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::WriteZero,
                        "zero-byte write while writing cache",
                    ),
                });
            }
            offset += written as u64;
            buf = &buf[written..];
        }
        Ok(())
    }
}

impl FuseCachePort for SparseFsCache {
    fn sync_metadata_placeholders(
        &self,
        dir_path: &Path,
        entries: &[CachedDirEntry],
    ) -> Result<()> {
        let metadata_dir = self.cache_path(dir_path);
        std::fs::create_dir_all(&metadata_dir).map_err(|source| Error::CacheIo {
            operation: "create metadata cache directory",
            path: metadata_dir.clone(),
            source,
        })?;
        for entry in entries {
            let relative_path = if dir_path.as_os_str().is_empty() {
                PathBuf::from(&entry.file_name)
            } else {
                dir_path.join(&entry.file_name)
            };
            self.ensure_placeholder(&relative_path, entry.is_dir, entry.size)?;
        }
        Ok(())
    }

    fn read_range(&self, path: &Path, start: u64, end: u64) -> Result<Vec<u8>> {
        if !self.is_range_cached(path, start, end) {
            return Err(Error::CacheRangeNotCached {
                path: path.to_path_buf(),
                start,
                end,
            });
        }
        let data_path = self.cache_path(path);
        let file = OpenOptions::new()
            .read(true)
            .open(&data_path)
            .map_err(|source| Error::CacheIo {
                operation: "open data cache",
                path: data_path.clone(),
                source,
            })?;
        let mut buf = vec![0u8; (end - start) as usize];
        Self::read_exact_at(&data_path, &file, &mut buf, start)?;
        Ok(buf)
    }

    fn write_range(&self, path: &Path, start: u64, data: &[u8], size: u64) -> Result<()> {
        let data_path = self.cache_path(path);
        Self::ensure_sparse_file(&data_path, size)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&data_path)
            .map_err(|source| Error::CacheIo {
                operation: "open data cache",
                path: data_path.clone(),
                source,
            })?;
        Self::write_all_at(&data_path, &file, data, start)?;
        self.mark_range_cached(path, start, start + data.len() as u64);
        Ok(())
    }
}

impl<S: Storage, L: Logger + 'static> FuseStorageFilesystem<S, L> {
    pub fn new(storage: S, cache_root: PathBuf, logger: L) -> Result<Self> {
        let cache = Arc::new(SparseFsCache::new(cache_root)?);
        Ok(Self::new_with_cache(storage, cache, logger))
    }
}

#[cfg(test)]
mod tests {
    use super::SparseFsCache;
    use crate::drivers::fuse::CachedDirEntry;
    use crate::drivers::fuse::StorageFilesystem;
    use crate::storages::{LocalStorage, Storage, WriteAt};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::SystemTime;
    use tempfile::TempDir;

    #[derive(Clone)]
    struct TestDirEntry {
        file_name: String,
        is_dir: bool,
        size: u64,
        accessed: SystemTime,
    }

    impl crate::storages::DirEntry for TestDirEntry {
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

    struct CountingStorage {
        read_dir_calls: Arc<AtomicUsize>,
        read_file_calls: Arc<AtomicUsize>,
    }

    impl Storage for CountingStorage {
        type Entry = TestDirEntry;
        type Iter = std::vec::IntoIter<TestDirEntry>;

        fn read_dir(&self, _path: PathBuf) -> crate::storages::Result<Self::Iter> {
            self.read_dir_calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![TestDirEntry {
                file_name: "alpha.txt".to_string(),
                is_dir: false,
                size: 5,
                accessed: SystemTime::UNIX_EPOCH,
            }]
            .into_iter())
        }

        fn read_file_at(
            &self,
            _path: PathBuf,
            _writer: &mut impl WriteAt,
            _range: std::ops::Range<u64>,
        ) -> crate::storages::Result<()> {
            self.read_file_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn temp_cache_root() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    struct MockCachePort {
        sync_calls: Arc<AtomicUsize>,
        data: Mutex<HashMap<PathBuf, Vec<u8>>>,
    }

    impl crate::drivers::fuse::CachePort for MockCachePort {
        fn sync_metadata_placeholders(
            &self,
            _dir_path: &Path,
            _entries: &[CachedDirEntry],
        ) -> crate::drivers::fuse::Result<()> {
            self.sync_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn read_range(
            &self,
            path: &Path,
            start: u64,
            end: u64,
        ) -> crate::drivers::fuse::Result<Vec<u8>> {
            let data = self.data.lock().unwrap();
            let Some(cached) = data.get(path) else {
                return Err(crate::drivers::fuse::Error::CacheRangeNotCached {
                    path: path.to_path_buf(),
                    start,
                    end,
                });
            };
            Ok(cached[start as usize..end as usize].to_vec())
        }

        fn write_range(
            &self,
            path: &Path,
            start: u64,
            payload: &[u8],
            size: u64,
        ) -> crate::drivers::fuse::Result<()> {
            let mut data = self.data.lock().unwrap();
            let entry = data
                .entry(path.to_path_buf())
                .or_insert_with(|| vec![0u8; size as usize]);
            let start = start as usize;
            let end = start + payload.len();
            entry[start..end].copy_from_slice(payload);
            Ok(())
        }
    }

    #[test]
    fn read_dir_entries_uses_cache_within_ttl() {
        let read_dir_calls = Arc::new(AtomicUsize::new(0));
        let read_file_calls = Arc::new(AtomicUsize::new(0));
        let storage = CountingStorage {
            read_dir_calls: Arc::clone(&read_dir_calls),
            read_file_calls,
        };
        let cache_root = temp_cache_root();
        let logger = crate::NoOpLogger;
        let fs = StorageFilesystem::new(storage, cache_root.path().to_path_buf(), logger).unwrap();
        let path = PathBuf::new();

        let first = fs.read_dir_entries(path.clone()).unwrap();
        let second = fs.read_dir_entries(path).unwrap();

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(read_dir_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn read_dir_entries_create_sparse_placeholders() {
        let read_dir_calls = Arc::new(AtomicUsize::new(0));
        let read_file_calls = Arc::new(AtomicUsize::new(0));
        let storage = CountingStorage {
            read_dir_calls,
            read_file_calls,
        };
        let cache_root = temp_cache_root();
        let logger = crate::NoOpLogger;
        let fs = StorageFilesystem::new(storage, cache_root.path().to_path_buf(), logger).unwrap();

        let entries = fs.read_dir_entries(PathBuf::new()).unwrap();
        assert_eq!(entries.len(), 1);

        let cache_file = cache_root.path().join("alpha.txt");
        assert!(cache_file.exists());
        assert_eq!(std::fs::metadata(cache_file).unwrap().len(), 5);
    }

    #[test]
    fn data_cache_marks_downloaded_ranges() {
        let read_dir_calls = Arc::new(AtomicUsize::new(0));
        let read_file_calls = Arc::new(AtomicUsize::new(0));
        let storage = CountingStorage {
            read_dir_calls,
            read_file_calls: Arc::clone(&read_file_calls),
        };
        let cache_root = temp_cache_root();
        let cache = SparseFsCache::new(cache_root.path().to_path_buf()).unwrap();

        cache
            .write_range(Path::new("alpha.txt"), 0, b"hello", 5)
            .unwrap();
        let data = cache.read_range(Path::new("alpha.txt"), 0, 5).unwrap();

        assert_eq!(data, b"hello");
        assert_eq!(read_file_calls.load(Ordering::SeqCst), 0);
        drop(storage);
    }

    #[test]
    fn read_dir_entries_call_cache_port() {
        let read_dir_calls = Arc::new(AtomicUsize::new(0));
        let read_file_calls = Arc::new(AtomicUsize::new(0));
        let storage = CountingStorage {
            read_dir_calls: Arc::clone(&read_dir_calls),
            read_file_calls,
        };
        let sync_calls = Arc::new(AtomicUsize::new(0));
        let cache = Arc::new(MockCachePort {
            sync_calls: Arc::clone(&sync_calls),
            data: Mutex::new(HashMap::new()),
        });
        let logger = crate::NoOpLogger;
        let fs = StorageFilesystem::new_with_cache(storage, cache, logger);

        let entries = fs.read_dir_entries(PathBuf::new()).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(sync_calls.load(Ordering::SeqCst), 1);
        assert_eq!(read_dir_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn sparse_cache_new_wraps_cache_io_error() {
        let err = SparseFsCache::new(PathBuf::from("/proc/anymount-denied"))
            .expect_err("cache init should fail");

        assert!(matches!(err, crate::drivers::linux::Error::CacheIo { .. }));
    }
}
