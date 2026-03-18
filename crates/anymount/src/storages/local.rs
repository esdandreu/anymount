use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::storage::{DirEntry, Storage, WriteAt};
use super::{Error, Result};

pub const DEFAULT_LOCAL_CHUNK_SIZE: usize = 65536;

fn io_error(path: &Path, source: std::io::Error) -> Error {
    if source.kind() == std::io::ErrorKind::UnexpectedEof {
        Error::UnexpectedEof {
            path: path.to_path_buf(),
        }
    } else {
        Error::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

fn read_exact_at(file: &std::fs::File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
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
    root: PathBuf,
    chunk_size: usize,
}

pub struct LocalDirEntry {
    file_name: String,
    is_dir: bool,
    size: u64,
    accessed: SystemTime,
}

impl LocalStorage {
    pub fn new(root: PathBuf) -> Self {
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

    fn read_dir(&self, path: PathBuf) -> Result<Self::Iter> {
        let full_path = self.root.join(path);
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&full_path).map_err(|source| io_error(&full_path, source))? {
            let entry = entry.map_err(|source| io_error(&full_path, source))?;
            let meta = entry
                .metadata()
                .map_err(|source| io_error(&entry.path(), source))?;
            let accessed = meta.accessed().unwrap_or(SystemTime::UNIX_EPOCH);
            entries.push(LocalDirEntry {
                file_name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                size: meta.len(),
                accessed,
            });
        }
        Ok(entries.into_iter())
    }

    fn read_file_at(
        &self,
        path: PathBuf,
        writer: &mut impl WriteAt,
        range: std::ops::Range<u64>,
    ) -> Result<()> {
        let full_path = self.root.join(path);
        let file =
            std::fs::File::open(&full_path).map_err(|source| io_error(&full_path, source))?;
        let len = (range.end - range.start) as usize;
        let chunk_size = self.chunk_size.min(len);
        let mut buf = vec![0u8; chunk_size];
        let mut pos = range.start;
        let end = range.end;
        while pos < end {
            let to_read = (end - pos).min(buf.len() as u64) as usize;
            read_exact_at(&file, &mut buf[..to_read], pos)
                .map_err(|source| io_error(&full_path, source))?;
            writer.write_at(&buf[..to_read], pos)?;
            pos += to_read as u64;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        fn write_at(&mut self, buf: &[u8], offset: u64) -> Result<()> {
            self.writes.push((offset, buf.to_vec()));
            Ok(())
        }
    }

    #[test]
    fn new_and_with_chunk_size() {
        let storage = LocalStorage::new(PathBuf::from("/tmp")).with_chunk_size(4096);
        let _ = storage;
    }

    #[test]
    fn read_dir_returns_entries() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        let file_path = path.join("f.txt");
        let content = b"hello world";
        std::fs::write(&file_path, content).unwrap();
        let subdir = path.join("sub");
        std::fs::create_dir(&subdir).unwrap();

        let storage = LocalStorage::new(path.to_path_buf());
        let iter = storage.read_dir(PathBuf::new()).unwrap();
        let entries: Vec<_> = iter.collect();
        assert!(entries.len() >= 2);

        let file_entry = entries.iter().find(|e| e.file_name() == "f.txt").unwrap();
        assert_eq!(file_entry.file_name(), "f.txt");
        assert!(!file_entry.is_dir());
        assert_eq!(file_entry.size(), content.len() as u64);

        let dir_entry = entries.iter().find(|e| e.file_name() == "sub").unwrap();
        assert!(dir_entry.is_dir());
    }

    #[test]
    fn read_file_at_writes_exact_range() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        let body: Vec<u8> = (0..5000).map(|i| (i % 256) as u8).collect();
        let file_path = path.join("f");
        std::fs::write(&file_path, &body).unwrap();

        let storage = LocalStorage::new(path.to_path_buf());
        let mut writer = RecordingWriter::new();
        storage
            .read_file_at(PathBuf::from("f"), &mut writer, 0..5000)
            .unwrap();
        assert_eq!(writer.total_bytes(), 5000);
        assert_eq!(writer.flat_bytes(), body);
    }

    #[test]
    fn read_file_at_caps_at_range() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        let body: Vec<u8> = (0..10_000).map(|i| (i % 256) as u8).collect();
        let file_path = path.join("f");
        std::fs::write(&file_path, &body).unwrap();

        let storage = LocalStorage::new(path.to_path_buf());
        let mut writer = RecordingWriter::new();
        storage
            .read_file_at(PathBuf::from("f"), &mut writer, 0..5000)
            .unwrap();
        assert_eq!(writer.total_bytes(), 5000);
    }
}
