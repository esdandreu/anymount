use std::path::PathBuf;
use std::time::SystemTime;

use cap_std::ambient_authority;
use cap_std::fs::Dir;

use crate::domain::{
    DirEntry, ReadDirError, ReadFileAtError, Storage, StoragePath,
};
use futures::io::{AsyncWrite, AsyncWriteExt};

pub const DEFAULT_LOCAL_CHUNK_SIZE: usize = 65536;

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
    ) -> Result<
        Box<
            dyn Iterator<Item = Result<Box<dyn DirEntry>, ReadDirError>> + Send,
        >,
        ReadDirError,
    > {
        let root = self.root.as_ref().map_err(root_io_error)?;
        let read_dir = if path.as_path().as_os_str().is_empty() {
            root.entries()
        } else {
            root.read_dir(path.as_path())
        };
        Ok(Box::new(read_dir.map_err(read_dir_error)?.map(|entry| {
            let entry = entry.map_err(read_dir_error)?;
            let meta = entry.metadata().map_err(read_dir_error)?;
            let accessed = meta
                .accessed()
                .map(cap_std::time::SystemTime::into_std)
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Ok(Box::new(LocalDirEntry {
                file_name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                size: meta.len(),
                accessed,
            }) as Box<dyn DirEntry>)
        })))
    }
}

#[async_trait::async_trait(?Send)]
impl Storage for LocalStorage {
    async fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<crate::domain::DirStream, ReadDirError> {
        let storage = self.clone();
        let entries =
            tokio::task::spawn_blocking(move || storage.read_dir(path))
                .await
                .map_err(|e| ReadDirError::Unknown {
                    message: e.to_string(),
                })??;
        Ok(Box::pin(futures::stream::unfold(
            Some(entries),
            |state| async move {
                let mut entries = state?;
                match tokio::task::spawn_blocking(move || {
                    let entry = entries.next();
                    (entry, entries)
                })
                .await
                {
                    Ok((Some(entry), entries)) => Some((entry, Some(entries))),
                    Ok((None, _)) => None,
                    Err(error) => Some((
                        Err(ReadDirError::Unknown {
                            message: error.to_string(),
                        }),
                        None,
                    )),
                }
            },
        )))
    }
    async fn read_file_at(
        &self,
        path: StoragePath,
        writer: &mut (dyn AsyncWrite + Unpin),
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
            let length = (range.end - offset).min(
                self.chunk_size
                    .max(1)
                    .min(crate::domain::MAX_READ_FILE_AT_RANGE_SIZE)
                    as u64,
            ) as usize;
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
            writer
                .write_all(&bytes)
                .await
                .map_err(ReadFileAtError::Write)?;
            offset += bytes.len() as u64;
        }
        Ok(offset - range.start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::mocks::RecordingWriter;
    use crate::{read_dir, read_file_at};
    use tempfile::TempDir;

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
        let entries: Vec<_> = futures::TryStreamExt::try_collect(entries)
            .await
            .expect("entries");
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
    async fn range_reads_await_partial_pending_writes_without_seeking_sink() {
        let dir = TempDir::new().expect("temporary directory");
        std::fs::write(dir.path().join("file"), [0, 1, 2, 3, 4, 5])
            .expect("write");
        let storage =
            LocalStorage::new(dir.path().to_path_buf()).with_chunk_size(3);
        let mut writer = RecordingWriter::new();
        writer.bytes.push(9);
        writer.max_write = 1;
        writer.pending = true;
        let count = storage
            .read_file_at(storage_path("file"), &mut writer, 2..6)
            .await
            .expect("range");
        assert_eq!(count, 4);
        assert_eq!(writer.bytes, [9, 2, 3, 4, 5]);
        assert_eq!(writer.pending_polls, 4);
        assert_eq!((writer.flushes, writer.closes), (0, 0));

        let mut cursor = futures::io::Cursor::new(vec![9; 6]);
        cursor.set_position(1);
        crate::read_file_at(&storage, PathBuf::from("file"), &mut cursor, 2..5)
            .await
            .expect("read at sink cursor");
        assert_eq!(cursor.position(), 4);
        assert_eq!(cursor.into_inner(), [9, 2, 3, 4, 9, 9]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn range_reads_preserve_sink_errors_and_reject_zero_writes() {
        let dir = TempDir::new().expect("temporary directory");
        std::fs::write(dir.path().join("file"), [0, 1, 2, 3, 4, 5])
            .expect("write");
        let storage =
            LocalStorage::new(dir.path().to_path_buf()).with_chunk_size(3);
        for zero in [false, true] {
            let mut writer = RecordingWriter::new();
            writer.max_write = 1;
            writer.pending = true;
            if zero {
                writer.zero_after = Some(2);
            } else {
                writer.fail_after = Some(2);
            }
            let error = crate::read_file_at(
                &storage,
                PathBuf::from("file"),
                &mut writer,
                1..6,
            )
            .await
            .expect_err("sink must fail");
            let ReadFileAtError::Write(source) = error else {
                panic!("expected sink error, got {error:?}");
            };
            assert_eq!(
                source.kind(),
                if zero {
                    std::io::ErrorKind::WriteZero
                } else {
                    std::io::ErrorKind::PermissionDenied
                }
            );
            if !zero {
                assert_eq!(source.to_string(), "test sink failure");
            }
            assert_eq!(writer.bytes, [1, 2]);
            assert_eq!((writer.flushes, writer.closes), (0, 0));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ranges_compose_with_non_send_sessions_without_finalizing() {
        use crate::test_utils::mocks::{MemoryWriteStorage, WriteSessionState};
        use futures::io::AsyncWriteExt;
        use std::{cell::RefCell, rc::Rc};

        let dir = TempDir::new().expect("temporary directory");
        std::fs::write(dir.path().join("file"), [0, 1, 2, 3, 4, 5])
            .expect("write");
        let storage = LocalStorage::new(dir.path().to_path_buf());
        for short in [false, true] {
            let state = Rc::new(RefCell::new(WriteSessionState::default()));
            let destination = MemoryWriteStorage(&state);
            let mut session = destination
                .start_write_file(storage_path("copy"))
                .await
                .expect("start");
            session.write_all(&[9]).await.expect("prefix");
            let result = crate::read_file_at(
                &storage,
                PathBuf::from("file"),
                session.as_mut(),
                2..if short { 8 } else { 6 },
            )
            .await;
            if short {
                assert!(matches!(
                    result,
                    Err(ReadFileAtError::RangeNotSatisfiable)
                ));
            } else {
                result.expect("copy range");
            }
            assert_eq!(state.borrow().staged, [9, 2, 3, 4, 5]);
            assert!(state.borrow().committed.is_none());
            assert!(
                state.borrow().events.iter().all(|event| *event == "write")
            );
            session.write_all(&[8]).await.expect("sink stays open");
            if short {
                session.abort().await.expect("caller abort");
                assert!(state.borrow().committed.is_none());
                assert!(state.borrow().staged.is_empty());
            } else {
                session.finish().await.expect("caller commit");
                assert_eq!(
                    state.borrow().committed.as_deref(),
                    Some([9, 2, 3, 4, 5, 8].as_slice())
                );
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn large_sparse_reads_are_chunked_and_writer_errors_stop_reads() {
        let dir = TempDir::new().expect("temporary directory");
        let start = 1_u64 << 40;
        let _file = crate::test_utils::create_sparse_file(
            &dir.path().join("sparse"),
            start + 131_073,
        )
        .expect("create sparse file");
        let storage = LocalStorage::new(dir.path().to_path_buf());
        let path = storage_path("sparse");
        let mut writer = crate::test_utils::mocks::CountingWriter::new();
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
        assert!(writer.max_chunk <= crate::domain::MAX_READ_FILE_AT_RANGE_SIZE);
        assert_eq!(writer.calls, 3);
        let mut writer = crate::test_utils::mocks::CountingWriter::new();
        writer.fail_after = Some(2);
        let result =
            Storage::read_file_at(&storage, path, &mut writer, 0..start).await;
        assert!(matches!(result, Err(ReadFileAtError::Write(_))));
        assert_eq!(writer.calls, 2);
        assert_eq!(
            writer.total,
            2 * crate::domain::MAX_READ_FILE_AT_RANGE_SIZE as u64
        );
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
        let entries: Vec<_> = iter.collect::<Result<_, _>>().expect("entries");
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
            .collect::<Result<_, _>>()
            .expect("entries");

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

    #[tokio::test(flavor = "current_thread")]
    async fn read_file_at_writes_exact_range() {
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
            .await
            .expect("test range should be readable");
        assert_eq!(writer.total_bytes(), 5000);
        assert_eq!(writer.flat_bytes(), body);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_file_at_allows_nested_file() {
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
            .await
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
    #[tokio::test(flavor = "current_thread")]
    async fn operations_reject_symlinks_outside_root() {
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
        let file = storage
            .read_file_at(storage_path("outside-file"), &mut writer, 0..6)
            .await;

        assert!(matches!(directory, Err(ReadDirError::PermissionDenied)));
        assert!(matches!(file, Err(ReadFileAtError::PermissionDenied)));
        assert_eq!(writer.total_bytes(), 0);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn operations_allow_symlinks_inside_root() {
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
            .collect::<Result<_, _>>()
            .expect("entries");
        storage
            .read_file_at(storage_path("internal-file"), &mut writer, 0..6)
            .await
            .expect("internal file symlink should be readable");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name(), "f.txt");
        assert_eq!(writer.flat_bytes(), b"inside");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_file_at_caps_at_range() {
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
            .await
            .expect("test range should be readable");
        assert_eq!(writer.total_bytes(), 5000);
    }
}
