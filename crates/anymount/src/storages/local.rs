use std::path::PathBuf;
use std::time::SystemTime;

use cap_std::ambient_authority;
use cap_std::fs::Dir;

#[cfg(test)]
use crate::domain::WriteAt;
use crate::domain::{
    DirEntry, ReadDirError, ReadFileAtError, Storage, StoragePath,
};

pub const DEFAULT_LOCAL_CHUNK_SIZE: usize = 65536;

/// Reads the exact number of bytes required to fill buf from the given offset.
/// The offset is relative to the start of the file and thus independent from
/// the current cursor.
#[cfg(test)]
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

#[derive(Clone)]
pub struct LocalStorage {
    root: Result<std::sync::Arc<Dir>, RootOpenError>,
    chunk_size: usize,
}

#[derive(Clone, Debug)]
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
        let root = Dir::open_ambient_dir(root, ambient_authority())
            .map(std::sync::Arc::new)
            .map_err(|source| RootOpenError {
                kind: source.kind(),
                message: source.to_string(),
            });
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

#[cfg(test)]
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
    fn size(&self) -> Option<u64> {
        Some(self.size)
    }
    fn accessed(&self) -> Option<SystemTime> {
        Some(self.accessed)
    }
}

impl LocalStorage {
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

    #[cfg(test)]
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

#[async_trait::async_trait(?Send)]
impl Storage for LocalStorage {
    async fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<Vec<Box<dyn DirEntry>>, ReadDirError> {
        let storage = self.clone();
        tokio::task::spawn_blocking(move || {
            storage.read_dir(path).map(|entries| entries.collect())
        })
        .await
        .map_err(|e| ReadDirError::Unknown {
            message: e.to_string(),
        })?
    }
    async fn read_file_at(
        &self,
        path: StoragePath,
        writer: &mut dyn crate::domain::WriteAt,
        range: std::ops::Range<u64>,
    ) -> Result<u64, ReadFileAtError> {
        if range.start > range.end {
            return Err(ReadFileAtError::Unknown {
                message: "invalid range".into(),
            });
        }
        if range.is_empty() {
            return Ok(0);
        }
        let storage = self.clone();
        let file = tokio::task::spawn_blocking(move || {
            let root = storage.root.as_ref().map_err(root_io_error)?;
            let file =
                root.open(path.as_path()).map_err(ReadFileAtError::from)?;
            Ok::<_, ReadFileAtError>(std::sync::Arc::new(file.into_std()))
        })
        .await
        .map_err(|e| ReadFileAtError::Unknown {
            message: e.to_string(),
        })??;
        let mut offset = range.start;
        while offset < range.end {
            let length = (range.end - offset)
                .min(self.chunk_size.max(1).min(crate::domain::MAX_READ_SIZE)
                    as u64) as usize;
            let file = file.clone();
            let bytes = tokio::task::spawn_blocking(move || {
                let mut bytes = vec![0; length];
                #[cfg(unix)]
                let count = {
                    use std::os::unix::fs::FileExt;
                    file.read_at(&mut bytes, offset)
                };
                #[cfg(windows)]
                let count = {
                    use std::os::windows::fs::FileExt;
                    file.seek_read(&mut bytes, offset)
                };
                let count = count.map_err(ReadFileAtError::from)?;
                bytes.truncate(count);
                Ok::<_, ReadFileAtError>(bytes)
            })
            .await
            .map_err(|e| ReadFileAtError::Unknown {
                message: e.to_string(),
            })??;
            if bytes.is_empty() {
                break;
            }
            writer.write_at(&bytes, offset)?;
            offset += bytes.len() as u64;
        }
        Ok(offset - range.start)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::WriteAtError;
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

    // private test util: Records writes in local storage tests.
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

    #[tokio::test(flavor = "current_thread")]
    async fn async_storage_lists_and_reads_ranges() {
        let dir = TempDir::new().expect("temporary directory");
        std::fs::write(dir.path().join("note"), [0, 1, 254, 255])
            .expect("write");
        let storage = LocalStorage::new(dir.path().to_path_buf());
        let entries = Storage::read_dir(&storage, StoragePath::root())
            .await
            .expect("list");
        let entry = entries
            .iter()
            .find(|entry| entry.file_name() == "note")
            .expect("entry");
        assert_eq!(entry.size(), Some(4));
        let path = storage_path("note");
        let mut writer = RecordingWriter::new();
        let count =
            Storage::read_file_at(&storage, path.clone(), &mut writer, 1..3)
                .await
                .expect("range");
        assert_eq!(count, 2);
        assert_eq!(writer.flat_bytes(), [1, 254]);
        let mut writer = RecordingWriter::new();
        assert_eq!(
            Storage::read_file_at(&storage, path.clone(), &mut writer, 3..7)
                .await
                .expect("short range"),
            1
        );
        assert_eq!(writer.flat_bytes(), [255]);
        assert_eq!(
            Storage::read_file_at(&storage, path.clone(), &mut writer, 4..5)
                .await
                .expect("EOF"),
            0
        );
        assert!(
            Storage::read_file_at(&storage, path, &mut writer, 2..1)
                .await
                .is_err()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn large_sparse_reads_are_chunked_and_writer_errors_stop_reads() {
        let dir = TempDir::new().expect("temporary directory");
        let file = std::fs::File::create(dir.path().join("sparse"))
            .expect("create sparse file");
        let start = 1_u64 << 40;
        file.set_len(start + 131_073).expect("extend sparse file");
        let storage = LocalStorage::new(dir.path().to_path_buf());
        let path = storage_path("sparse");
        let mut writer = crate::test_utils::mocks::CountingWriter::new(start);
        let count = Storage::read_file_at(
            &storage,
            path.clone(),
            &mut writer,
            start..start + 131_073,
        )
        .await
        .expect("read sparse range");
        assert_eq!(count, 131_073);
        assert_eq!(writer.total, count);
        assert!(writer.max_chunk <= crate::domain::MAX_READ_SIZE);
        assert_eq!(writer.calls, 3);
        let mut writer = crate::test_utils::mocks::CountingWriter::new(0);
        writer.fail_after = Some(2);
        let result =
            Storage::read_file_at(&storage, path, &mut writer, 0..start).await;
        assert!(matches!(result, Err(ReadFileAtError::WriteAt(_))));
        assert_eq!(writer.calls, 2);
        assert_eq!(writer.total, 2 * crate::domain::MAX_READ_SIZE as u64);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn async_listing_preserves_missing_and_wrong_type_errors() {
        let dir = TempDir::new().expect("temporary directory");
        std::fs::write(dir.path().join("file"), b"data").expect("write file");
        let storage = LocalStorage::new(dir.path().to_path_buf());
        assert!(matches!(
            Storage::read_dir(&storage, storage_path("missing")).await,
            Err(ReadDirError::NotFound)
        ));
        assert!(matches!(
            Storage::read_dir(&storage, storage_path("file")).await,
            Err(ReadDirError::NotADirectory)
        ));
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn async_range_does_not_follow_symlinks_outside_root() {
        use std::os::unix::fs::symlink;
        let root = TempDir::new().expect("root");
        let outside = TempDir::new().expect("outside");
        std::fs::write(outside.path().join("secret"), b"secret")
            .expect("write");
        symlink(outside.path().join("secret"), root.path().join("link"))
            .expect("symlink");
        let storage = LocalStorage::new(root.path().to_path_buf());
        assert!(
            Storage::read_file_at(
                &storage,
                storage_path("link"),
                &mut RecordingWriter::new(),
                0..6
            )
            .await
            .is_err()
        );
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
        assert_eq!(file_entry.size(), Some(content.len() as u64));

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

    #[tokio::test(flavor = "current_thread")]
    async fn read_dir_rejects_absolute_path() {
        let dir =
            TempDir::new().expect("temporary directory should be created");
        let storage = LocalStorage::new(dir.path().to_path_buf());

        let result = read_dir(&storage, dir.path().to_path_buf()).await;

        assert!(matches!(result, Err(crate::ReadDirError::InvalidPath(_))));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_dir_rejects_parent_components() {
        let dir =
            TempDir::new().expect("temporary directory should be created");
        let storage = LocalStorage::new(dir.path().to_path_buf());

        let parent = read_dir(&storage, PathBuf::from("..")).await;
        let nested =
            read_dir(&storage, PathBuf::from("nested/../../outside")).await;

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

    #[tokio::test(flavor = "current_thread")]
    async fn read_file_at_rejects_absolute_path() {
        let dir =
            TempDir::new().expect("temporary directory should be created");
        let storage = LocalStorage::new(dir.path().to_path_buf());
        let mut writer = RecordingWriter::new();

        let result = read_file_at(
            &storage,
            dir.path().join("outside"),
            &mut writer,
            0..1,
        )
        .await;

        assert!(matches!(
            result,
            Err(crate::ReadFileAtError::InvalidPath(_))
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_file_at_rejects_parent_components() {
        let dir =
            TempDir::new().expect("temporary directory should be created");
        let storage = LocalStorage::new(dir.path().to_path_buf());
        let mut writer = RecordingWriter::new();

        let parent = read_file_at(
            &storage,
            PathBuf::from("../outside"),
            &mut writer,
            0..1,
        )
        .await;
        let nested = read_file_at(
            &storage,
            PathBuf::from("nested/../../outside"),
            &mut writer,
            0..1,
        )
        .await;

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
