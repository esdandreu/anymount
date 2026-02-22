//! Read-only FUSE filesystem backed by the [`Storage`] trait.

use crate::storages::{DirEntry, Storage, WriteAt};
use fuser::{
    Errno, FileAttr, FileType, Generation, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request,
};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const ROOT_INO: u64 = 1;
const TTL: Duration = Duration::from_secs(1);
const FUSE_GENERATION: Generation = Generation(1);

/// Metadata for a single node (file or directory) in the FUSE tree.
struct NodeInfo {
    path: PathBuf,
    is_dir: bool,
    size: u64,
    atime: SystemTime,
}

/// Read-only FUSE filesystem that delegates to a [`Storage`] implementation.
pub struct StorageFilesystem<S: Storage> {
    storage: Arc<S>,
    next_ino: AtomicU64,
    ino_to_info: RwLock<HashMap<u64, NodeInfo>>,
    path_to_ino: RwLock<HashMap<PathBuf, u64>>,
}

impl<S: Storage> StorageFilesystem<S> {
    pub fn new(storage: S) -> Self {
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
            next_ino,
            ino_to_info,
            path_to_ino,
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

    fn attr_from_info(&self, ino: u64, info: &NodeInfo) -> FileAttr {
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
}

impl<S: Storage> fuser::Filesystem for StorageFilesystem<S> {
    fn lookup(
        &self,
        _req: &Request,
        parent: u64,
        name: &std::ffi::OsStr,
        reply: ReplyEntry,
    ) {
        let parent_info = match self.get_info(parent) {
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
            let (ino, info) = if parent == ROOT_INO {
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
            let attr = self.attr_from_info(ino, &info);
            reply.entry(&TTL, &attr, FUSE_GENERATION);
            return;
        }
        let child_path = if parent_info.path.as_os_str().is_empty() {
            PathBuf::from(name)
        } else {
            parent_info.path.join(name)
        };
        let entries: Vec<_> = match self.storage.read_dir(parent_info.path.clone()) {
            Ok(iter) => iter.collect(),
            Err(_) => {
                reply.error(Errno::from_i32(libc::ENOENT));
                return;
            }
        };
        let entry = match entries.iter().find(|e| e.file_name() == name_str.as_ref()) {
            Some(e) => e,
            None => {
                reply.error(Errno::from_i32(libc::ENOENT));
                return;
            }
        };
        let is_dir = entry.is_dir();
        let size = entry.size();
        let atime = entry.accessed();
        let ino = self.get_or_create_ino(child_path.clone(), is_dir, size, atime);
        let info = self.get_info(ino).unwrap();
        let attr = self.attr_from_info(ino, &info);
        reply.entry(&TTL, &attr, FUSE_GENERATION);
    }

    fn getattr(
        &self,
        _req: &Request,
        ino: u64,
        _fh: Option<u64>,
        reply: ReplyAttr,
    ) {
        match self.get_info(ino) {
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
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        let info = match self.get_info(ino) {
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
        let offset = offset as u64;
        let end = (offset + size as u64).min(info.size);
        if offset >= end {
            reply.data(&[]);
            return;
        }
        let range_len = (end - offset) as usize;
        struct VecWriter {
            buf: Vec<u8>,
            range_start: u64,
        }
        impl WriteAt for VecWriter {
            fn write_at(&mut self, buf: &[u8], at: u64) -> Result<(), String> {
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
        reply.data(&writer.buf);
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        reply: ReplyDirectory,
    ) {
        let info = match self.get_info(ino) {
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
        let entries: Vec<_> = match self.storage.read_dir(info.path.clone()) {
            Ok(iter) => iter.collect(),
            Err(_) => {
                reply.error(Errno::from_i32(libc::EIO));
                return;
            }
        };
        let mut offset = offset as u64;
        if offset == 0 {
            if reply.add(ROOT_INO, offset, FileType::Directory, ".") {
                reply.ok();
                return;
            }
            offset += 1;
        }
        if offset == 1 {
            let parent_ino = if info.path.as_os_str().is_empty() {
                ROOT_INO
            } else {
                *self.path_to_ino.read().get(&info.path.parent().unwrap_or(std::path::Path::new()).to_path_buf()).unwrap_or(&ROOT_INO)
            };
            let parent_info = self.get_info(parent_ino).unwrap();
            if reply.add(parent_ino, offset, FileType::Directory, "..") {
                reply.ok();
                return;
            }
            offset += 1;
        }
        let mut idx = (offset - 2) as usize;
        for entry in entries.iter().skip(idx) {
            let child_path = if info.path.as_os_str().is_empty() {
                PathBuf::from(entry.file_name())
            } else {
                info.path.join(entry.file_name())
            };
            let kind = if entry.is_dir() {
                FileType::Directory
            } else {
                FileType::RegularFile
            };
            let child_ino = self.get_or_create_ino(
                child_path,
                entry.is_dir(),
                entry.size(),
                entry.accessed(),
            );
            if reply.add(child_ino, offset, kind, entry.file_name()) {
                reply.ok();
                return;
            }
            offset += 1;
            idx += 1;
        }
        reply.ok();
    }
}
