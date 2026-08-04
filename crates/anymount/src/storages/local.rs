use std::path::PathBuf;
use std::time::SystemTime;

use cap_std::ambient_authority;
use cap_std::fs::Dir;

use crate::domain::storage::{
    DirEntry, ReadDirError, ReadFileAtError, Storage, StoragePath, WriteAt,
};

pub const DEFAULT_LOCAL_CHUNK_SIZE: usize = 65536;

/// Reads the exact number of bytes required to fill buf from the given offset.
/// The offset is relative to the start of the file and thus independent from
/// the current cursor.
fn read_exact_at(
    file: &std::fs::File,
    buf: &mut [u8],
    offset: u64,
) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        FileExt::read_exact_at(file, buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut pos = 0;
        while pos < buf.len() {
            let n = file.seek_read(&mut buf[pos..], offset + pos as u64)?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "failed to fill whole buffer",
                ));
            }
            pos += n;
        }
        Ok(())
    }
}

pub struct LocalStorage {
    root: Result<Dir, RootOpenError>,
    chunk_size: usize,
}

#[derive(Debug)]
struct RootOpenError {
    kind: std::io::ErrorKind,
    message: String,
}

pub struct LocalDirEntry {
    file_name: String,
    is_dir: bool,
    size: u64,
    accessed: SystemTime,
}

impl LocalStorage {
    pub fn new(root: PathBuf) -> Self {
        let root = Dir::open_ambient_dir(root, ambient_authority()).map_err(
            |source| RootOpenError {
                kind: source.kind(),
                message: source.to_string(),
            },
        );
        Self {
            root,
            chunk_size: DEFAULT_LOCAL_CHUNK_SIZE,
        }
    }

    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }
}

fn root_io_error(error: &RootOpenError) -> std::io::Error {
    std::io::Error::new(error.kind, error.message.clone())
}

fn read_dir_error(source: std::io::Error) -> ReadDirError {
    if source.kind() == std::io::ErrorKind::PermissionDenied {
        ReadDirError::PermissionDenied
    } else {
        source.into()
    }
}

fn read_file_error(source: std::io::Error) -> ReadFileAtError {
    if source.kind() == std::io::ErrorKind::PermissionDenied {
        ReadFileAtError::PermissionDenied
    } else {
        source.into()
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
    fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<Box<dyn Iterator<Item = Box<dyn DirEntry>>>, ReadDirError> {
        let root = self.root.as_ref().map_err(root_io_error)?;
        let mut entries: Vec<Box<dyn DirEntry>> = Vec::new();
        let read_dir = if path.as_path().as_os_str().is_empty() {
            root.entries()
        } else {
            root.read_dir(path.as_path())
        };
        for entry in read_dir.map_err(read_dir_error)? {
            let entry = entry.map_err(read_dir_error)?;
            let meta = entry.metadata().map_err(read_dir_error)?;
            let accessed = meta
                .accessed()
                .map(cap_std::time::SystemTime::into_std)
                .unwrap_or(SystemTime::UNIX_EPOCH);
            entries.push(Box::new(LocalDirEntry {
                file_name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                size: meta.len(),
                accessed,
            }));
        }
        Ok(Box::new(entries.into_iter()))
    }

    fn read_file_at(
        &self,
        path: StoragePath,
        writer: &mut dyn WriteAt,
        range: std::ops::Range<u64>,
    ) -> Result<(), ReadFileAtError> {
        let root = self.root.as_ref().map_err(root_io_error)?;
        let file = root.open(path.as_path()).map_err(read_file_error)?;
        let file = file.into_std();
        let len = (range.end - range.start) as usize;
        let chunk_size = self.chunk_size.min(len);
        let mut buf = vec![0u8; chunk_size];
        let mut pos = range.start;
        let end = range.end;
        while pos < end {
            let to_read = (end - pos).min(buf.len() as u64) as usize;
            read_exact_at(&file, &mut buf[..to_read], pos)?;
            writer.write_at(&buf[..to_read], pos)?;
            pos += to_read as u64;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::storage::WriteAtError;
    use crate::{read_dir, read_file_at};
    use tempfile::TempDir;

    struct RecordingWriter {
        writes: Vec<(u64, Vec<u8>)>,
    }

    impl RecordingWriter {
        fn new() -> Self {
            Self { writes: Vec::new() }
        }
        fn total_bytes(&self) -> u64 {
            self.writes.iter().map(|(_, b)| b.len() as u64).sum()
        }
        fn flat_bytes(&self) -> Vec<u8> {
            self.writes
                .iter()
                .flat_map(|(_, b)| b.iter().copied())
                .collect()
        }
    }

    impl WriteAt for RecordingWriter {
        fn write_at(
            &mut self,
            buf: &[u8],
            offset: u64,
        ) -> Result<(), WriteAtError> {
            self.writes.push((offset, buf.to_vec()));
            Ok(())
        }
    }

    fn storage_path(value: impl Into<PathBuf>) -> StoragePath {
        StoragePath::try_from(value.into())
            .expect("test storage path should be valid")
    }

    #[test]
    fn new_and_with_chunk_size() {
        let storage =
            LocalStorage::new(PathBuf::from("/tmp")).with_chunk_size(4096);
        let _ = storage;
    }

    #[test]
    fn read_dir_returns_entries() {
        let dir =
            TempDir::new().expect("temporary directory should be created");
        let path = dir.path();
        let file_path = path.join("f.txt");
        let content = b"hello world";
        std::fs::write(&file_path, content)
            .expect("test file should be written");
        let subdir = path.join("sub");
        std::fs::create_dir(&subdir)
            .expect("test subdirectory should be created");

        let storage = LocalStorage::new(path.to_path_buf());
        let iter = storage
            .read_dir(StoragePath::root())
            .expect("test directory should be readable");
        let entries: Vec<_> = iter.collect();
        assert!(entries.len() >= 2);

        let file_entry = entries
            .iter()
            .find(|e| e.file_name() == "f.txt")
            .expect("test file entry should exist");
        assert_eq!(file_entry.file_name(), "f.txt");
        assert!(!file_entry.is_dir());
        assert_eq!(file_entry.size(), content.len() as u64);

        let dir_entry = entries
            .iter()
            .find(|e| e.file_name() == "sub")
            .expect("test directory entry should exist");
        assert!(dir_entry.is_dir());
    }

    #[test]
    fn read_dir_allows_nested_directory() {
        let dir =
            TempDir::new().expect("temporary directory should be created");
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested)
            .expect("nested directory should be created");
        std::fs::write(nested.join("f.txt"), b"nested")
            .expect("nested test file should be written");

        let storage = LocalStorage::new(dir.path().to_path_buf());
        let entries: Vec<_> = storage
            .read_dir(storage_path("nested"))
            .expect("nested directory should be readable")
            .collect();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name(), "f.txt");
    }

    #[test]
    fn read_dir_rejects_absolute_path() {
        let dir =
            TempDir::new().expect("temporary directory should be created");
        let storage = LocalStorage::new(dir.path().to_path_buf());

        let result = read_dir(&storage, dir.path().to_path_buf());

        assert!(matches!(result, Err(crate::ReadDirError::InvalidPath(_))));
    }

    #[test]
    fn read_dir_rejects_parent_components() {
        let dir =
            TempDir::new().expect("temporary directory should be created");
        let storage = LocalStorage::new(dir.path().to_path_buf());

        let parent = read_dir(&storage, PathBuf::from(".."));
        let nested = read_dir(&storage, PathBuf::from("nested/../../outside"));

        assert!(matches!(parent, Err(crate::ReadDirError::InvalidPath(_))));
        assert!(matches!(nested, Err(crate::ReadDirError::InvalidPath(_))));
    }

    #[test]
    fn read_file_at_writes_exact_range() {
        let dir =
            TempDir::new().expect("temporary directory should be created");
        let path = dir.path();
        let body: Vec<u8> = (0..5000).map(|i| (i % 256) as u8).collect();
        let file_path = path.join("f");
        std::fs::write(&file_path, &body).expect("test file should be written");

        let storage = LocalStorage::new(path.to_path_buf());
        let mut writer = RecordingWriter::new();
        storage
            .read_file_at(storage_path("f"), &mut writer, 0..5000)
            .expect("test range should be readable");
        assert_eq!(writer.total_bytes(), 5000);
        assert_eq!(writer.flat_bytes(), body);
    }

    #[test]
    fn read_file_at_allows_nested_file() {
        let dir =
            TempDir::new().expect("temporary directory should be created");
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested)
            .expect("nested directory should be created");
        std::fs::write(nested.join("f.txt"), b"nested")
            .expect("nested test file should be written");
        let storage = LocalStorage::new(dir.path().to_path_buf());
        let mut writer = RecordingWriter::new();

        storage
            .read_file_at(storage_path("nested/f.txt"), &mut writer, 0..6)
            .expect("nested file should be readable");

        assert_eq!(writer.flat_bytes(), b"nested");
    }

    #[test]
    fn read_file_at_rejects_absolute_path() {
        let dir =
            TempDir::new().expect("temporary directory should be created");
        let storage = LocalStorage::new(dir.path().to_path_buf());
        let mut writer = RecordingWriter::new();

        let result = read_file_at(
            &storage,
            dir.path().join("outside"),
            &mut writer,
            0..1,
        );

        assert!(matches!(
            result,
            Err(crate::ReadFileAtError::InvalidPath(_))
        ));
    }

    #[test]
    fn read_file_at_rejects_parent_components() {
        let dir =
            TempDir::new().expect("temporary directory should be created");
        let storage = LocalStorage::new(dir.path().to_path_buf());
        let mut writer = RecordingWriter::new();

        let parent = read_file_at(
            &storage,
            PathBuf::from("../outside"),
            &mut writer,
            0..1,
        );
        let nested = read_file_at(
            &storage,
            PathBuf::from("nested/../../outside"),
            &mut writer,
            0..1,
        );

        assert!(matches!(
            parent,
            Err(crate::ReadFileAtError::InvalidPath(_))
        ));
        assert!(matches!(
            nested,
            Err(crate::ReadFileAtError::InvalidPath(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn operations_reject_symlinks_outside_root() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().expect("storage root should be created");
        let outside =
            TempDir::new().expect("outside directory should be created");
        std::fs::write(outside.path().join("secret"), b"secret")
            .expect("outside test file should be written");
        symlink(outside.path(), root.path().join("outside-dir"))
            .expect("outside directory symlink should be created");
        symlink(
            outside.path().join("secret"),
            root.path().join("outside-file"),
        )
        .expect("outside file symlink should be created");
        let storage = LocalStorage::new(root.path().to_path_buf());
        let mut writer = RecordingWriter::new();

        let directory = storage.read_dir(storage_path("outside-dir"));
        let file = storage.read_file_at(
            storage_path("outside-file"),
            &mut writer,
            0..6,
        );

        assert!(matches!(directory, Err(ReadDirError::PermissionDenied)));
        assert!(matches!(file, Err(ReadFileAtError::PermissionDenied)));
        assert_eq!(writer.total_bytes(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn operations_allow_symlinks_inside_root() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().expect("storage root should be created");
        let nested = root.path().join("nested");
        std::fs::create_dir(&nested)
            .expect("nested directory should be created");
        std::fs::write(nested.join("f.txt"), b"inside")
            .expect("nested test file should be written");
        symlink("nested", root.path().join("internal-dir"))
            .expect("internal directory symlink should be created");
        symlink("nested/f.txt", root.path().join("internal-file"))
            .expect("internal file symlink should be created");
        let storage = LocalStorage::new(root.path().to_path_buf());
        let mut writer = RecordingWriter::new();

        let entries: Vec<_> = storage
            .read_dir(storage_path("internal-dir"))
            .expect("internal directory symlink should be readable")
            .collect();
        storage
            .read_file_at(storage_path("internal-file"), &mut writer, 0..6)
            .expect("internal file symlink should be readable");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name(), "f.txt");
        assert_eq!(writer.flat_bytes(), b"inside");
    }

    #[test]
    fn read_file_at_caps_at_range() {
        let dir =
            TempDir::new().expect("temporary directory should be created");
        let path = dir.path();
        let body: Vec<u8> = (0..10_000).map(|i| (i % 256) as u8).collect();
        let file_path = path.join("f");
        std::fs::write(&file_path, &body).expect("test file should be written");

        let storage = LocalStorage::new(path.to_path_buf());
        let mut writer = RecordingWriter::new();
        storage
            .read_file_at(storage_path("f"), &mut writer, 0..5000)
            .expect("test range should be readable");
        assert_eq!(writer.total_bytes(), 5000);
    }
}
