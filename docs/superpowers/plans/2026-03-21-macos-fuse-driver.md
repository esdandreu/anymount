# macOS FUSE Driver Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add macOS support via cross-platform FUSE driver using the `fuser` crate with on-demand reads (no caching).

**Architecture:** Extract shared FUSE implementation (CachePort trait, StorageFilesystem) into `drivers/fuse/`. Both Linux and macOS use this shared code. Linux provides SparseFsCache, macOS uses NoCacheFsCache.

**Tech Stack:** fuser crate, Rust

---

## File Structure

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `drivers/fuse/mod.rs` | CachePort trait, NoCacheFsCache, shared StorageFilesystem |
| Create | `drivers/fuse/error.rs` | Fuse-specific errors (CacheRangeNotCached, CacheIo, FuseMount) |
| Modify | `drivers/linux/fuse.rs` | Linux-only SparseFsCache, imports StorageFilesystem from fuse |
| Modify | `drivers/linux/mod.rs` | Re-export from fuse |
| Modify | `drivers/mod.rs` | Add `cfg(target_os = "macos")` module, macOS connect_drivers |
| Modify | `drivers/driver.rs` | Add macOS connect_drivers implementation |
| Modify | `Cargo.toml` | Add fuser, libc for macOS target |
| Modify | `drivers/error.rs` | Add macOS error variant |

---

## Task 1: Create fuse error types

**Files:**
- Create: `crates/anymount/src/drivers/fuse/error.rs`

- [ ] **Step 1: Create fuse/error.rs**

```rust
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("cache I/O failed: {operation} {path}: {source}")]
    CacheIo {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cache range not cached: {path} [{start}..{end})")]
    CacheRangeNotCached {
        path: PathBuf,
        start: u64,
        end: u64,
    },
    #[error("unexpected EOF reading cache: {path}")]
    CacheUnexpectedEof { path: PathBuf },
    #[error("FUSE mount failed at {path}: {source}")]
    FuseMount {
        path: PathBuf,
        #[source]
        source: fuser::Errno,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/src/drivers/fuse/error.rs
git commit -m "feat(fuse): add error types"
```

---

## Task 2: Create fuse module with CachePort and NoCacheFsCache

**Files:**
- Create: `crates/anymount/src/drivers/fuse/mod.rs`

- [ ] **Step 1: Create fuse/mod.rs with CachePort trait and NoCacheFsCache**

```rust
//! Shared FUSE filesystem implementation.

pub mod error;

pub use error::{Error, Result};

use crate::storages::{DirEntry, Storage, WriteAt};
use crate::Logger;
use error::Error::CacheRangeNotCached;
use fuser::{
    Errno, FileAttr, FileHandle, FileType, Generation, INodeNo, OpenFlags, ReplyAttr, ReplyData,
    ReplyDirectory, ReplyEntry, Request,
};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

const ROOT_INO: u64 = 1;
const TTL: Duration = Duration::from_secs(1);
const DIR_CACHE_TTL: Duration = Duration::from_secs(2);
const FUSE_GENERATION: Generation = Generation(1);
const DOT_ENTRY_OFFSET: u64 = 1;
const DOT_DOT_ENTRY_OFFSET: u64 = 2;
const FIRST_CHILD_ENTRY_OFFSET: u64 = 3;

#[derive(Clone)]
pub struct NodeInfo {
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub atime: SystemTime,
}

#[derive(Clone)]
pub struct CachedDirEntry {
    pub file_name: String,
    pub is_dir: bool,
    pub size: u64,
    pub accessed: SystemTime,
}

#[derive(Clone)]
pub struct CachedDir {
    pub entries: Vec<CachedDirEntry>,
    loaded_at: Instant,
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
        let cache = self.cache.read();
        let entry = cache.get(path)?;
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
}

impl Default for CachedDirCache {
    fn default() -> Self {
        Self::new()
    }
}

pub trait CachePort: Send + Sync {
    fn sync_metadata_placeholders(
        &self,
        dir_path: &Path,
        entries: &[CachedDirEntry],
    ) -> Result<()>;
    fn read_range(&self, path: &Path, start: u64, end: u64) -> Result<Vec<u8>>;
    fn write_range(&self, path: &Path, start: u64, data: &[u8], size: u64) -> Result<()>;
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
        Err(CacheRangeNotCached {
            path: path.to_path_buf(),
            start,
            end,
        })
    }

    fn write_range(&self, _path: &Path, _start: u64, _data: &[u8], _size: u64) -> Result<()> {
        Ok(())
    }
}

pub struct StorageFilesystem<S: Storage, L: Logger + 'static> {
    storage: Arc<S>,
    cache: Arc<dyn CachePort>,
    logger: L,
    next_ino: AtomicU64,
    ino_to_info: RwLock<HashMap<u64, NodeInfo>>,
    path_to_ino: RwLock<HashMap<PathBuf, u64>>,
    dir_cache: CachedDirCache,
}

impl<S: Storage, L: Logger + 'static> StorageFilesystem<S, L> {
    pub fn new(storage: S, cache: Arc<dyn CachePort>, logger: L) -> Self {
        let storage = Arc::new(storage);
        let next_ino = AtomicU64::new(2);
        let ino_to_info = RwLock::new(HashMap::new());
        let path_to_ino = RwLock::new(HashMap::new());
        {
            let now = SystemTime::now();
            ino_to_info.write().insert(
                ROOT_INO,
                NodeInfo {
                    path: PathBuf::new(),
                    is_dir: true,
                    size: 0,
                    atime: now,
                },
            );
            path_to_ino.write().insert(PathBuf::new(), ROOT_INO);
        }
        Self {
            storage,
            cache,
            logger,
            next_ino,
            ino_to_info,
            path_to_ino,
            dir_cache: CachedDirCache::new(),
        }
    }

    pub fn get_info(&self, ino: u64) -> Option<NodeInfo> {
        self.ino_to_info.read().get(&ino).cloned()
    }

    pub fn get_or_create_ino(
        &self,
        path: PathBuf,
        is_dir: bool,
        size: u64,
        atime: SystemTime,
    ) -> u64 {
        {
            let guard = self.path_to_ino.read();
            if let Some(&ino) = guard.get(&path) {
                return ino;
            }
        }
        let ino = self.next_ino.fetch_add(1, Ordering::SeqCst);
        self.ino_to_info.write().insert(
            ino,
            NodeInfo {
                path: path.clone(),
                is_dir,
                size,
                atime,
            },
        );
        self.path_to_ino.write().insert(path, ino);
        ino
    }

    fn attr_from_info(&self, ino: INodeNo, info: &NodeInfo) -> FileAttr {
        let kind = if info.is_dir {
            FileType::Directory
        } else {
            FileType::RegularFile
        };
        let perm = if info.is_dir { 0o755 } else { 0o644 };
        FileAttr {
            ino,
            size: info.size,
            blocks: (info.size + 511) / 512,
            atime: info.atime,
            mtime: info.atime,
            ctime: info.atime,
            crtime: info.atime,
            kind,
            perm,
            nlink: 1,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            blksize: 512,
            flags: 0,
        }
    }

    pub fn child_entry_offset(index: usize) -> u64 {
        index as u64 + FIRST_CHILD_ENTRY_OFFSET
    }

    pub fn child_start_index(offset: u64) -> usize {
        offset.saturating_sub(DOT_DOT_ENTRY_OFFSET) as usize
    }

    pub fn read_dir_entries(&self, path: PathBuf) -> Result<Vec<CachedDirEntry>> {
        if let Some(entries) = self.dir_cache.get(&path) {
            return Ok(entries);
        }
        let entries: Vec<CachedDirEntry> = self
            .storage
            .read_dir(path.clone())?
            .map(|entry| CachedDirEntry {
                file_name: entry.file_name(),
                is_dir: entry.is_dir(),
                size: entry.size(),
                accessed: entry.accessed(),
            })
            .collect();
        self.cache.sync_metadata_placeholders(&path, &entries)?;
        self.dir_cache.insert(path, entries.clone());
        Ok(entries)
    }
}

impl<S: Storage, L: Logger + 'static> fuser::Filesystem for StorageFilesystem<S, L> {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &std::ffi::OsStr, reply: ReplyEntry) {
        let parent_info = match self.get_info(parent.0) {
            Some(i) => i,
            None => {
                reply.error(Errno::from_i32(libc::ENOENT));
                return;
            }
        };
        let name_os = name.to_os_string();
        let name_str = name_os.to_string_lossy();
        if name_str == "." {
            let attr = self.attr_from_info(parent, &parent_info);
            reply.entry(&TTL, &attr, FUSE_GENERATION);
            return;
        }
        if name_str == ".." {
            let (ino, info) = if parent.0 == ROOT_INO {
                (ROOT_INO, self.get_info(ROOT_INO).unwrap())
            } else {
                let grandparent = parent_info
                    .path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_default();
                let ino = *self
                    .path_to_ino
                    .read()
                    .get(&grandparent)
                    .unwrap_or(&ROOT_INO);
                let info = self.get_info(ino).unwrap();
                (ino, info)
            };
            let attr = self.attr_from_info(INodeNo(ino), &info);
            reply.entry(&TTL, &attr, FUSE_GENERATION);
            return;
        }
        let child_path = if parent_info.path.as_os_str().is_empty() {
            PathBuf::from(name)
        } else {
            parent_info.path.join(name)
        };
        let entries = match self.read_dir_entries(parent_info.path.clone()) {
            Ok(entries) => entries,
            Err(_) => {
                reply.error(Errno::from_i32(libc::ENOENT));
                return;
            }
        };
        let entry = match entries.iter().find(|e| e.file_name == name_str.as_ref()) {
            Some(e) => e,
            None => {
                reply.error(Errno::from_i32(libc::ENOENT));
                return;
            }
        };
        let is_dir = entry.is_dir;
        let size = entry.size;
        let atime = entry.accessed;
        let ino = self.get_or_create_ino(child_path.clone(), is_dir, size, atime);
        let info = self.get_info(ino).unwrap();
        let attr = self.attr_from_info(INodeNo(ino), &info);
        reply.entry(&TTL, &attr, FUSE_GENERATION);
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        match self.get_info(ino.0) {
            Some(info) => {
                let attr = self.attr_from_info(ino, &info);
                reply.attr(&TTL, &attr);
            }
            None => reply.error(Errno::from_i32(libc::ENOENT)),
        }
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<fuser::LockOwner>,
        reply: ReplyData,
    ) {
        let info = match self.get_info(ino.0) {
            Some(i) => i,
            None => {
                reply.error(Errno::from_i32(libc::ENOENT));
                return;
            }
        };
        if info.is_dir {
            reply.error(Errno::from_i32(libc::EISDIR));
            return;
        }
        let end = (offset + size as u64).min(info.size);
        if offset >= end {
            reply.data(&[]);
            return;
        }
        if let Ok(buf) = self.cache.read_range(&info.path, offset, end) {
            self.logger.debug(format!(
                "served read from cache path={} offset={} size={}",
                info.path.display(),
                offset,
                end - offset
            ));
            reply.data(&buf);
            return;
        }
        let range_len = (end - offset) as usize;
        struct VecWriter {
            buf: Vec<u8>,
            range_start: u64,
        }
        impl WriteAt for VecWriter {
            fn write_at(&mut self, buf: &[u8], at: u64) -> crate::storages::Result<()> {
                let start = (at.saturating_sub(self.range_start)) as usize;
                let end = start + buf.len();
                if end > self.buf.len() {
                    self.buf.resize(end, 0);
                }
                self.buf[start..end].copy_from_slice(buf);
                Ok(())
            }
        }
        let mut writer = VecWriter {
            buf: vec![0u8; range_len],
            range_start: offset,
        };
        if self
            .storage
            .read_file_at(info.path.clone(), &mut writer, offset..end)
            .is_err()
        {
            reply.error(Errno::from_i32(libc::EIO));
            return;
        }
        if let Err(err) = self
            .cache
            .write_range(&info.path, offset, &writer.buf, info.size)
        {
            self.logger.warn(format!(
                "failed to write cache path={} error={}",
                info.path.display(),
                err
            ));
        }
        reply.data(&writer.buf);
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let info = match self.get_info(ino.0) {
            Some(i) => i,
            None => {
                reply.error(Errno::from_i32(libc::ENOENT));
                return;
            }
        };
        if !info.is_dir {
            reply.error(Errno::from_i32(libc::ENOTDIR));
            return;
        }
        let entries = match self.read_dir_entries(info.path.clone()) {
            Ok(entries) => entries,
            Err(err) => {
                self.logger.warn(format!(
                    "readdir failed to list storage path path={} error={}",
                    info.path.display(),
                    err
                ));
                reply.error(Errno::from_i32(libc::EIO));
                return;
            }
        };
        if offset == 0 {
            if reply.add(ino, DOT_ENTRY_OFFSET, FileType::Directory, ".") {
                reply.ok();
                return;
            }
        }
        if offset <= DOT_ENTRY_OFFSET {
            let parent_ino = if info.path.as_os_str().is_empty() {
                ROOT_INO
            } else {
                *self
                    .path_to_ino
                    .read()
                    .get(
                        &info
                            .path
                            .parent()
                            .unwrap_or(std::path::Path::new(""))
                            .to_path_buf(),
                    )
                    .unwrap_or(&ROOT_INO)
            };
            if reply.add(
                INodeNo(parent_ino),
                DOT_DOT_ENTRY_OFFSET,
                FileType::Directory,
                "..",
            ) {
                reply.ok();
                return;
            }
        }
        let idx = Self::child_start_index(offset);
        for (entry_index, entry) in entries.iter().enumerate().skip(idx) {
            let child_path = if info.path.as_os_str().is_empty() {
                PathBuf::from(&entry.file_name)
            } else {
                info.path.join(&entry.file_name)
            };
            let kind = if entry.is_dir {
                FileType::Directory
            } else {
                FileType::RegularFile
            };
            let child_ino =
                self.get_or_create_ino(child_path, entry.is_dir, entry.size, entry.accessed);
            let next_offset = Self::child_entry_offset(entry_index);
            if reply.add(INodeNo(child_ino), next_offset, kind, &entry.file_name) {
                reply.ok();
                return;
            }
        }
        reply.ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storages::{Storage, WriteAt};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::SystemTime;

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

    struct MockCachePort {
        sync_calls: Arc<AtomicUsize>,
        data: Mutex<HashMap<PathBuf, Vec<u8>>>,
    }

    impl CachePort for MockCachePort {
        fn sync_metadata_placeholders(
            &self,
            _dir_path: &Path,
            _entries: &[CachedDirEntry],
        ) -> Result<()> {
            self.sync_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn read_range(&self, path: &Path, start: u64, end: u64) -> Result<Vec<u8>> {
            let data = self.data.lock().unwrap();
            let Some(cached) = data.get(path) else {
                return Err(CacheRangeNotCached {
                    path: path.to_path_buf(),
                    start,
                    end,
                });
            };
            Ok(cached[start as usize..end as usize].to_vec())
        }

        fn write_range(&self, path: &Path, start: u64, payload: &[u8], size: u64) -> Result<()> {
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
    fn child_start_index_maps_special_offsets() {
        assert_eq!(StorageFilesystem::<CountingStorage, crate::NoOpLogger>::child_start_index(0), 0);
        assert_eq!(
            StorageFilesystem::<CountingStorage, crate::NoOpLogger>::child_start_index(DOT_ENTRY_OFFSET),
            0
        );
        assert_eq!(
            StorageFilesystem::<CountingStorage, crate::NoOpLogger>::child_start_index(DOT_DOT_ENTRY_OFFSET),
            0
        );
        assert_eq!(
            StorageFilesystem::<CountingStorage, crate::NoOpLogger>::child_start_index(FIRST_CHILD_ENTRY_OFFSET),
            1
        );
    }

    #[test]
    fn child_entry_offset_is_monotonic_and_starts_after_special_entries() {
        assert_eq!(
            StorageFilesystem::<CountingStorage, crate::NoOpLogger>::child_entry_offset(0),
            FIRST_CHILD_ENTRY_OFFSET
        );
        assert_eq!(
            StorageFilesystem::<CountingStorage, crate::NoOpLogger>::child_entry_offset(1),
            FIRST_CHILD_ENTRY_OFFSET + 1
        );
        assert_eq!(
            StorageFilesystem::<CountingStorage, crate::NoOpLogger>::child_entry_offset(7),
            10
        );
    }

    #[test]
    fn read_dir_entries_uses_cache_within_ttl() {
        let read_dir_calls = Arc::new(AtomicUsize::new(0));
        let read_file_calls = Arc::new(AtomicUsize::new(0));
        let storage = CountingStorage {
            read_dir_calls: Arc::clone(&read_dir_calls),
            read_file_calls,
        };
        let cache = Arc::new(MockCachePort {
            sync_calls: Arc::new(AtomicUsize::new(0)),
            data: Mutex::new(HashMap::new()),
        });
        let logger = crate::NoOpLogger;
        let fs = StorageFilesystem::new(storage, cache, logger);
        let path = PathBuf::new();

        let first = fs.read_dir_entries(path.clone()).unwrap();
        let second = fs.read_dir_entries(path).unwrap();

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(read_dir_calls.load(Ordering::SeqCst), 1);
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
        let fs = StorageFilesystem::new(storage, cache, logger);

        let entries = fs.read_dir_entries(PathBuf::new()).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(sync_calls.load(Ordering::SeqCst), 1);
        assert_eq!(read_dir_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn no_cache_fs_cache_read_always_returns_error() {
        let cache = NoCacheFsCache::new();
        let result = cache.read_range(Path::new("test.txt"), 0, 10);
        assert!(matches!(result, Err(CacheRangeNotCached { .. })));
    }

    #[test]
    fn no_cache_fs_cache_write_always_succeeds() {
        let cache = NoCacheFsCache::new();
        let result = cache.write_range(Path::new("test.txt"), 0, b"hello", 5);
        assert!(result.is_ok());
    }

    #[test]
    fn no_cache_fs_cache_sync_always_succeeds() {
        let cache = NoCacheFsCache::new();
        let entries = vec![CachedDirEntry {
            file_name: "test.txt".to_string(),
            is_dir: false,
            size: 5,
            accessed: SystemTime::UNIX_EPOCH,
        }];
        let result = cache.sync_metadata_placeholders(Path::new("/"), &entries);
        assert!(result.is_ok());
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --features fuse -p anymount`
Expected: Should compile (with fuse feature or on Unix)

- [ ] **Step 3: Commit**

```bash
git add crates/anymount/src/drivers/fuse/
git commit -m "feat(fuse): add shared FUSE implementation with CachePort and NoCacheFsCache"
```

---

## Task 3: Refactor Linux driver to use fuse module

**Files:**
- Modify: `crates/anymount/src/drivers/linux/fuse.rs`

- [ ] **Step 1: Replace linux/fuse.rs with SparseFsCache and re-export**

```rust
//! Linux-specific FUSE filesystem with sparse cache.

use super::{Error, Result};
use crate::drivers::fuse::{
    CachedDirEntry, CachePort, NodeInfo, StorageFilesystem,
};
use crate::storages::{DirEntry, Storage};
use crate::Logger;
use fuser::{Errno, FileAttr, FileHandle, FileType, Generation, INodeNo, OpenFlags, ReplyAttr,
            ReplyData, ReplyDirectory, ReplyEntry, Request};
use parking_lot::RwLock;
use std::collections::{BTreeSet, HashMap};
use std::fs::OpenOptions;
use std::os::unix::fs::FileExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use crate::drivers::fuse::error::Error as FuseError;

const DATA_CACHE_BLOCK_SIZE: u64 = 64 * 1024;
const ROOT_INO: u64 = 1;
const TTL: Duration = Duration::from_secs(1);
const FUSE_GENERATION: Generation = Generation(1);
const DOT_ENTRY_OFFSET: u64 = 1;
const DOT_DOT_ENTRY_OFFSET: u64 = 2;
const FIRST_CHILD_ENTRY_OFFSET: u64 = 3;

#[derive(Debug)]
pub struct SparseFsCache {
    cache_root: PathBuf,
    data_cache_blocks: RwLock<HashMap<PathBuf, BTreeSet<u64>>>,
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
            data_cache_blocks: RwLock::new(HashMap::new()),
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

impl CachePort for SparseFsCache {
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
        use crate::drivers::fuse::error::Error as FuseErr;
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

impl From<Error> for crate::drivers::Error {
    fn from(err: Error) -> Self {
        crate::drivers::Error::LibCloudProvider(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn sparse_cache_new_wraps_cache_io_error() {
        let err = SparseFsCache::new(PathBuf::from("/proc/anymount-denied"))
            .expect_err("cache init should fail");
        assert!(matches!(err, Error::CacheIo { .. }));
    }

    #[test]
    fn data_cache_marks_downloaded_ranges() {
        let cache_root = tempfile::tempdir().unwrap();
        let cache = SparseFsCache::new(cache_root.path().to_path_buf()).unwrap();

        cache
            .write_range(Path::new("alpha.txt"), 0, b"hello", 5)
            .unwrap();
        let data = cache.read_range(Path::new("alpha.txt"), 0, 5).unwrap();

        assert_eq!(data, b"hello");
    }
}
```

- [ ] **Step 2: Update linux/mod.rs to re-export from fuse**

```rust
pub mod dbus;
pub mod error;
pub mod fuse;
pub mod gtk_dbus;
pub mod linux_driver;

pub use error::{Error, Result};
pub use fuse::{CachePort, CachedDirCache, CachedDirEntry, NodeInfo, NoCacheFsCache, StorageFilesystem};
pub use linux_driver::{export_on_dbus, mount_storage, new_runtime, LinuxDriver};
```

- [ ] **Step 3: Update linux_driver.rs to use fuse module**

Modify imports to use `crate::drivers::fuse::StorageFilesystem` instead of local fuse module.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p anymount --features linux`
Expected: Should compile

- [ ] **Step 5: Commit**

```bash
git add crates/anymount/src/drivers/linux/ crates/anymount/src/drivers/fuse/
git commit -m "refactor(fuse): extract shared FUSE implementation"
```

---

## Task 4: Add macOS support

**Files:**
- Modify: `crates/anymount/src/drivers/mod.rs`
- Modify: `crates/anymount/src/drivers/driver.rs`
- Modify: `Cargo.toml`
- Modify: `crates/anymount/src/drivers/error.rs`

- [ ] **Step 1: Add macOS feature and dependencies to Cargo.toml**

Add after the Linux dependencies section:

```toml
# macOS-specific dependencies
[target.'cfg(target_os = "macos")'.dependencies]
fuser = "0.17"
libc = "0.2"
```

- [ ] **Step 2: Add fuse module to drivers/mod.rs**

Add at the top of drivers/mod.rs:

```rust
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod fuse;
```

- [ ] **Step 3: Add macOS Error variant to drivers/error.rs**

```rust
#[cfg(target_os = "macos")]
#[error(transparent)]
Macos(#[from] crate::drivers::fuse::error::Error),
```

- [ ] **Step 4: Add macOS connect_drivers to driver.rs**

Add after the Linux cfg block:

```rust
#[cfg(target_os = "macos")]
pub fn connect_drivers(
    specs: &[DomainDriver],
    logger: &(impl Logger + 'static),
) -> Result<Vec<Box<dyn Driver>>> {
    connect_drivers_with_telemetry(specs, logger, None)
}

#[cfg(target_os = "macos")]
pub fn connect_drivers_with_telemetry(
    specs: &[DomainDriver],
    logger: &(impl Logger + 'static),
    _service_tx: Option<Sender<ServiceMessage>>,
) -> Result<Vec<Box<dyn Driver>>> {
    use super::fuse::{NoCacheFsCache, StorageFilesystem};
    use crate::drivers::fuse::error::Error as FuseError;
    let mut sessions: Vec<(std::path::PathBuf, fuser::BackgroundSession)> = Vec::new();
    for spec in specs {
        let path = spec.path.clone();
        match &spec.storage {
            StorageSpec::Local { root } => {
                let storage = LocalStorage::new(root.clone());
                let cache = Arc::new(NoCacheFsCache::new());
                let fs = StorageFilesystem::new(storage, cache, logger.clone());
                let session = fuser::spawn_mount2(fs, &path, &fuser::Config::default())
                    .map_err(|source| super::Error::Macos(FuseError::FuseMount {
                        path: path.clone(),
                        source,
                    }))?;
                sessions.push((path, session));
            }
            StorageSpec::OneDrive { .. } => {
                return Err(super::Error::NotSupported);
            }
        }
    }
    let drivers: Vec<Box<dyn Driver>> = sessions
        .into_iter()
        .map(|(path, session)| {
            Box::new(MacosDriver::new(path, session)) as Box<dyn Driver>
        })
        .collect();
    Ok(drivers)
}

#[cfg(target_os = "macos")]
use super::fuse::StorageFilesystem;

#[cfg(target_os = "macos")]
pub struct MacosDriver {
    path: std::path::PathBuf,
    _session: fuser::BackgroundSession,
}

#[cfg(target_os = "macos")]
impl MacosDriver {
    pub fn new(path: std::path::PathBuf, session: fuser::BackgroundSession) -> Self {
        Self {
            path,
            _session: session,
        }
    }
}

#[cfg(target_os = "macos")]
impl Driver for MacosDriver {
    fn kind(&self) -> &'static str {
        "fuse"
    }

    fn path(&self) -> &std::path::PathBuf {
        &self.path
    }
}
```

- [ ] **Step 4: Add Arc import**

At the top of `drivers/driver.rs`, ensure `use std::sync::Arc;` is imported.

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p anymount --features macos`
Expected: Should compile (may need platform-specific build)

- [ ] **Step 6: Commit**

```bash
git add crates/anymount/Cargo.toml crates/anymount/src/drivers/
git commit -m "feat(macos): add FUSE driver for macOS with on-demand reads"
```

---

## Task 5: Run full test suite

- [ ] **Step 1: Run tests**

Run: `cargo test -p anymount`
Expected: All tests pass

- [ ] **Step 2: Commit if tests pass**

```bash
git commit -m "test: verify FUSE driver tests pass"
```

---

## Task 6: Add macOS integration test

**Files:**
- Modify: `crates/anymount/tests/system/local_provider_test.rs`

- [ ] **Step 1: Add macOS-specific test section**

Add at the end of `local_provider_test.rs`:

```rust
#[cfg(target_os = "macos")]
mod macos_tests {
    use super::*;

    #[test]
    #[ignore = "requires macFUSE installed"]
    fn mount_local_directory_on_macos() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mount_path = temp_dir.path().join("mount");
        std::fs::create_dir(&mount_path).unwrap();

        let driver = crate::drivers::macos::MacosDriver::new(
            mount_path.clone(),
            // FUSE session would be created here in real usage
        );
        
        assert_eq!(driver.kind(), "fuse");
        assert_eq!(driver.path(), &mount_path);
    }
}
```

Note: Due to FUSE requiring kernel integration, integration tests for macOS will typically need to be run manually with macFUSE installed.

- [ ] **Step 2: Commit**

```bash
git add crates/anymount/tests/system/local_provider_test.rs
git commit -m "test(macos): add basic macOS driver test"
```

---

## Summary

| Task | Files | Description |
|------|-------|-------------|
| 1 | fuse/error.rs | Create error types for FUSE operations |
| 2 | fuse/mod.rs | CachePort trait, NoCacheFsCache, StorageFilesystem |
| 3 | linux/fuse.rs, linux/mod.rs | Refactor to use fuse module |
| 4 | drivers/mod.rs, drivers/driver.rs, drivers/error.rs, Cargo.toml | Add macOS support |
| 5 | - | Run tests |
| 6 | tests/system/ | Add macOS integration test |

**Note:** `MacosDriver` is defined inline in `drivers/driver.rs` rather than a separate `drivers/fuse/macos.rs` module, as requested.
