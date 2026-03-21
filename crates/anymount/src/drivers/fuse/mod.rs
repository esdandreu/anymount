// Copyright 2026 Dotphoton AG

use crate::drivers::fuse::error::{Error, Result};
use crate::storages::{DirEntry, Storage, WriteAt};
use crate::Logger;
use fuser::{
    Errno, FileAttr, FileHandle, FileType, Generation, INodeNo, OpenFlags, ReplyAttr, ReplyData,
    ReplyDirectory, ReplyEntry, Request,
};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

pub use crate::drivers::fuse::error::{Error as FuseError, Result as FuseResult};

pub const ROOT_INO: u64 = 1;
pub const TTL: Duration = Duration::from_secs(1);
const DIR_CACHE_TTL: Duration = Duration::from_secs(2);
pub const FUSE_GENERATION: Generation = Generation(1);
pub const DOT_ENTRY_OFFSET: u64 = 1;
pub const DOT_DOT_ENTRY_OFFSET: u64 = 2;
pub const FIRST_CHILD_ENTRY_OFFSET: u64 = 3;

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

pub trait CachePort: Send + Sync {
    fn sync_metadata_placeholders(&self, dir_path: &Path, entries: &[CachedDirEntry])
        -> Result<()>;
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
        Err(Error::CacheRangeNotCached {
            path: path.to_path_buf(),
            start,
            end,
        })
    }

    fn write_range(&self, _path: &Path, _start: u64, _data: &[u8], _size: u64) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct NodeInfo {
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub atime: SystemTime,
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
    pub fn new_with_cache(storage: S, cache: Arc<dyn CachePort>, logger: L) -> Self {
        let storage = Arc::new(storage);
        let next_ino = AtomicU64::new(2);
        let ino_to_info = RwLock::new(HashMap::new());
        let path_to_ino = RwLock::new(HashMap::new());
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

    fn get_or_create_ino(&self, path: PathBuf, is_dir: bool, size: u64, atime: SystemTime) -> u64 {
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
                "served read from local cache path={} offset={} size={}",
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
                "failed to write data cache path={} error={}",
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
    use super::{
        CachePort, CachedDirCache, CachedDirEntry, NoCacheFsCache, StorageFilesystem,
        DOT_DOT_ENTRY_OFFSET, DOT_ENTRY_OFFSET, FIRST_CHILD_ENTRY_OFFSET,
    };
    use crate::drivers::fuse::error::Error;
    use crate::storages::{Storage, WriteAt};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant, SystemTime};

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
        read_calls: Arc<AtomicUsize>,
        write_calls: Arc<AtomicUsize>,
    }

    impl MockCachePort {
        fn new() -> (Self, Arc<AtomicUsize>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
            let sync_calls = Arc::new(AtomicUsize::new(0));
            let read_calls = Arc::new(AtomicUsize::new(0));
            let write_calls = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    sync_calls: sync_calls.clone(),
                    read_calls: read_calls.clone(),
                    write_calls: write_calls.clone(),
                },
                sync_calls,
                read_calls,
                write_calls,
            )
        }
    }

    impl CachePort for MockCachePort {
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
            _path: &Path,
            _start: u64,
            _end: u64,
        ) -> crate::drivers::fuse::Result<Vec<u8>> {
            self.read_calls.fetch_add(1, Ordering::SeqCst);
            Err(Error::CacheRangeNotCached {
                path: PathBuf::new(),
                start: 0,
                end: 0,
            })
        }

        fn write_range(
            &self,
            _path: &Path,
            _start: u64,
            _data: &[u8],
            _size: u64,
        ) -> crate::drivers::fuse::Result<()> {
            self.write_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn no_cache_fs_cache_sync_metadata_returns_ok() {
        let cache = NoCacheFsCache::new();
        let entries = vec![CachedDirEntry {
            file_name: "test.txt".to_string(),
            is_dir: false,
            size: 100,
            accessed: SystemTime::now(),
        }];
        let result = cache.sync_metadata_placeholders(Path::new("/"), &entries);
        assert!(result.is_ok());
    }

    #[test]
    fn no_cache_fs_cache_read_range_always_fails() {
        let cache = NoCacheFsCache::new();
        let result = cache.read_range(Path::new("test.txt"), 0, 10);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::CacheRangeNotCached { .. }));
    }

    #[test]
    fn no_cache_fs_cache_write_range_always_returns_ok() {
        let cache = NoCacheFsCache::new();
        let result = cache.write_range(Path::new("test.txt"), 0, b"hello", 5);
        assert!(result.is_ok());
    }

    #[test]
    fn cached_dir_cache_returns_entries_within_ttl() {
        let cache = CachedDirCache::new();
        let entries = vec![CachedDirEntry {
            file_name: "test.txt".to_string(),
            is_dir: false,
            size: 100,
            accessed: SystemTime::now(),
        }];
        let path = PathBuf::from("/");
        cache.insert(path.clone(), entries.clone());
        let result = cache.get(&path);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn cached_dir_cache_returns_none_after_ttl() {
        let cache = CachedDirCache::new();
        let entries = vec![CachedDirEntry {
            file_name: "test.txt".to_string(),
            is_dir: false,
            size: 100,
            accessed: SystemTime::now(),
        }];
        let path = PathBuf::from("/");
        cache.insert(path.clone(), entries.clone());
        std::thread::sleep(Duration::from_secs(3));
        let result = cache.get(&path);
        assert!(result.is_none());
    }

    #[test]
    fn cached_dir_cache_invalidate_removes_entries() {
        let cache = CachedDirCache::new();
        let entries = vec![CachedDirEntry {
            file_name: "test.txt".to_string(),
            is_dir: false,
            size: 100,
            accessed: SystemTime::now(),
        }];
        let path = PathBuf::from("/");
        cache.insert(path.clone(), entries);
        cache.invalidate(&path);
        assert!(cache.get(&path).is_none());
    }

    #[test]
    fn cached_dir_cache_clear_removes_all() {
        let cache = CachedDirCache::new();
        let entries = vec![CachedDirEntry {
            file_name: "test.txt".to_string(),
            is_dir: false,
            size: 100,
            accessed: SystemTime::now(),
        }];
        cache.insert(PathBuf::from("/"), entries.clone());
        cache.insert(PathBuf::from("/subdir"), entries);
        cache.clear();
        assert!(cache.get(&PathBuf::from("/")).is_none());
        assert!(cache.get(&PathBuf::from("/subdir")).is_none());
    }

    #[test]
    fn child_start_index_maps_special_offsets() {
        assert_eq!(
            StorageFilesystem::<CountingStorage, crate::NoOpLogger>::child_start_index(0),
            0
        );
        assert_eq!(
            StorageFilesystem::<CountingStorage, crate::NoOpLogger>::child_start_index(
                DOT_ENTRY_OFFSET
            ),
            0
        );
        assert_eq!(
            StorageFilesystem::<CountingStorage, crate::NoOpLogger>::child_start_index(
                DOT_DOT_ENTRY_OFFSET
            ),
            0
        );
        assert_eq!(
            StorageFilesystem::<CountingStorage, crate::NoOpLogger>::child_start_index(
                FIRST_CHILD_ENTRY_OFFSET
            ),
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
    fn storage_filesystem_new_with_cache_creates_root_node() {
        let (mock_cache, _, _, _) = MockCachePort::new();
        let storage = CountingStorage {
            read_dir_calls: Arc::new(AtomicUsize::new(0)),
            read_file_calls: Arc::new(AtomicUsize::new(0)),
        };
        let cache = Arc::new(mock_cache);
        let logger = crate::NoOpLogger;
        let fs = StorageFilesystem::new_with_cache(storage, cache, logger);
        let root_info = fs.get_info(1);
        assert!(root_info.is_some());
        assert!(root_info.unwrap().is_dir);
    }

    #[test]
    fn read_dir_entries_uses_cache_within_ttl() {
        let read_dir_calls = Arc::new(AtomicUsize::new(0));
        let read_file_calls = Arc::new(AtomicUsize::new(0));
        let storage = CountingStorage {
            read_dir_calls: Arc::clone(&read_dir_calls),
            read_file_calls,
        };
        let (mock_cache, sync_calls, _, _) = MockCachePort::new();
        let cache = Arc::new(mock_cache);
        let logger = crate::NoOpLogger;
        let fs = StorageFilesystem::new_with_cache(storage, cache, logger);
        let path = PathBuf::new();

        let first = fs.read_dir_entries(path.clone()).unwrap();
        let second = fs.read_dir_entries(path).unwrap();

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(read_dir_calls.load(Ordering::SeqCst), 1);
        assert_eq!(sync_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn read_dir_entries_call_cache_port() {
        let read_dir_calls = Arc::new(AtomicUsize::new(0));
        let read_file_calls = Arc::new(AtomicUsize::new(0));
        let storage = CountingStorage {
            read_dir_calls: Arc::clone(&read_dir_calls),
            read_file_calls,
        };
        let (mock_cache, sync_calls, _, _) = MockCachePort::new();
        let cache = Arc::new(mock_cache);
        let logger = crate::NoOpLogger;
        let fs = StorageFilesystem::new_with_cache(storage, cache, logger);

        let entries = fs.read_dir_entries(PathBuf::new()).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(sync_calls.load(Ordering::SeqCst), 1);
        assert_eq!(read_dir_calls.load(Ordering::SeqCst), 1);
    }
}
