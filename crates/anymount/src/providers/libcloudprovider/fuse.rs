//! Read-only FUSE filesystem backed by the [`Storage`] trait.

use super::{Error, Result};
use crate::Logger;
use crate::storages::{DirEntry, Storage, WriteAt};
use fuser::{
    Errno, FileAttr, FileHandle, FileType, Generation, INodeNo, OpenFlags, ReplyAttr, ReplyData,
    ReplyDirectory, ReplyEntry, Request,
};
use parking_lot::RwLock;
use std::collections::{BTreeSet, HashMap};
use std::fs::OpenOptions;
use std::os::unix::fs::FileExt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

const ROOT_INO: u64 = 1;
const TTL: Duration = Duration::from_secs(1);
/// Cache list operations briefly to avoid repeated network requests per `ls`.
const DIR_CACHE_TTL: Duration = Duration::from_secs(2);
/// Data cache chunk size used to track downloaded sparse file blocks.
const DATA_CACHE_BLOCK_SIZE: u64 = 64 * 1024;
const FUSE_GENERATION: Generation = Generation(1);
const DOT_ENTRY_OFFSET: u64 = 1;
const DOT_DOT_ENTRY_OFFSET: u64 = 2;
const FIRST_CHILD_ENTRY_OFFSET: u64 = 3;

/// Metadata for a single node (file or directory) in the FUSE tree.
#[derive(Clone)]
struct NodeInfo {
    path: PathBuf,
    is_dir: bool,
    size: u64,
    atime: SystemTime,
}

#[derive(Clone)]
struct CachedDirEntry {
    file_name: String,
    is_dir: bool,
    size: u64,
    accessed: SystemTime,
}

#[derive(Clone)]
struct CachedDir {
    entries: Vec<CachedDirEntry>,
    loaded_at: Instant,
}

trait CachePort: Send + Sync {
    fn sync_metadata_placeholders(&self, dir_path: &Path, entries: &[CachedDirEntry])
    -> Result<()>;
    fn read_range(&self, path: &Path, start: u64, end: u64) -> Result<Vec<u8>>;
    fn write_range(&self, path: &Path, start: u64, data: &[u8], size: u64) -> Result<()>;
}

struct SparseFsCache {
    cache_root: PathBuf,
    data_cache_blocks: RwLock<HashMap<PathBuf, BTreeSet<u64>>>,
}

impl SparseFsCache {
    fn new(cache_root: PathBuf) -> Result<Self> {
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

/// Read-only FUSE filesystem that delegates to a [`Storage`] implementation.
pub struct StorageFilesystem<S: Storage, L: Logger + 'static> {
    storage: Arc<S>,
    cache: Arc<dyn CachePort>,
    logger: L,
    next_ino: AtomicU64,
    ino_to_info: RwLock<HashMap<u64, NodeInfo>>,
    path_to_ino: RwLock<HashMap<PathBuf, u64>>,
    dir_cache: RwLock<HashMap<PathBuf, CachedDir>>,
}

impl<S: Storage, L: Logger + 'static> StorageFilesystem<S, L> {
    pub fn new(storage: S, cache_root: PathBuf, logger: L) -> Result<Self> {
        let cache = Arc::new(SparseFsCache::new(cache_root)?);
        Ok(Self::new_with_cache(storage, cache, logger))
    }

    fn new_with_cache(storage: S, cache: Arc<dyn CachePort>, logger: L) -> Self {
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
            dir_cache: RwLock::new(HashMap::new()),
        }
    }

    fn get_info(&self, ino: u64) -> Option<NodeInfo> {
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

    fn child_entry_offset(index: usize) -> u64 {
        index as u64 + FIRST_CHILD_ENTRY_OFFSET
    }

    fn child_start_index(offset: u64) -> usize {
        offset.saturating_sub(DOT_DOT_ENTRY_OFFSET) as usize
    }

    fn get_cached_dir_entries(&self, path: &PathBuf) -> Option<Vec<CachedDirEntry>> {
        let cache = self.dir_cache.read();
        let entry = cache.get(path)?;
        if entry.loaded_at.elapsed() > DIR_CACHE_TTL {
            return None;
        }
        Some(entry.entries.clone())
    }

    fn read_dir_entries(&self, path: PathBuf) -> Result<Vec<CachedDirEntry>> {
        if let Some(entries) = self.get_cached_dir_entries(&path) {
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
        self.dir_cache.write().insert(
            path,
            CachedDir {
                entries: entries.clone(),
                loaded_at: Instant::now(),
            },
        );
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
                "served read from local sparse cache path={} offset={} size={}",
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
        CachePort, DOT_DOT_ENTRY_OFFSET, DOT_ENTRY_OFFSET, FIRST_CHILD_ENTRY_OFFSET, Result,
        SparseFsCache, StorageFilesystem,
    };
    use crate::storages::{LocalStorage, Storage, WriteAt};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
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

    impl CachePort for MockCachePort {
        fn sync_metadata_placeholders(
            &self,
            _dir_path: &Path,
            _entries: &[super::CachedDirEntry],
        ) -> Result<()> {
            self.sync_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn read_range(&self, path: &Path, start: u64, end: u64) -> Result<Vec<u8>> {
            let data = self.data.lock().unwrap();
            let Some(cached) = data.get(path) else {
                return Err(super::Error::CacheRangeNotCached {
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
        assert_eq!(
            StorageFilesystem::<LocalStorage, crate::NoOpLogger>::child_start_index(0),
            0
        );
        assert_eq!(
            StorageFilesystem::<LocalStorage, crate::NoOpLogger>::child_start_index(
                DOT_ENTRY_OFFSET
            ),
            0
        );
        assert_eq!(
            StorageFilesystem::<LocalStorage, crate::NoOpLogger>::child_start_index(
                DOT_DOT_ENTRY_OFFSET
            ),
            0
        );
        assert_eq!(
            StorageFilesystem::<LocalStorage, crate::NoOpLogger>::child_start_index(
                FIRST_CHILD_ENTRY_OFFSET
            ),
            1
        );
    }

    #[test]
    fn child_entry_offset_is_monotonic_and_starts_after_special_entries() {
        assert_eq!(
            StorageFilesystem::<LocalStorage, crate::NoOpLogger>::child_entry_offset(0),
            FIRST_CHILD_ENTRY_OFFSET
        );
        assert_eq!(
            StorageFilesystem::<LocalStorage, crate::NoOpLogger>::child_entry_offset(1),
            FIRST_CHILD_ENTRY_OFFSET + 1
        );
        assert_eq!(
            StorageFilesystem::<LocalStorage, crate::NoOpLogger>::child_entry_offset(7),
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

        assert!(matches!(
            err,
            crate::providers::libcloudprovider::Error::CacheIo { .. }
        ));
    }
}
