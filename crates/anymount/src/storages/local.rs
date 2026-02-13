use std::path::PathBuf;
use std::time::SystemTime;

use super::storage::{DirEntry, Storage, WriteAt};

pub const DEFAULT_LOCAL_CHUNK_SIZE: usize = 65536;

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

    fn read_dir(&self, path: PathBuf) -> std::result::Result<Self::Iter, String> {
        let full_path = self.root.join(path);
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&full_path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let meta = entry.metadata().map_err(|e| e.to_string())?;
            let accessed = meta
                .accessed()
                .unwrap_or(SystemTime::UNIX_EPOCH);
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
    ) -> std::result::Result<(), String> {
        let full_path = self.root.join(path);
        let file = std::fs::File::open(&full_path).map_err(|e| e.to_string())?;
        let len = (range.end - range.start) as usize;
        let chunk_size = self.chunk_size.min(len);
        let mut buf = vec![0u8; chunk_size];
        let mut pos = range.start;
        let end = range.end;
        while pos < end {
            let to_read = (end - pos).min(buf.len() as u64) as usize;
            read_exact_at(&file, &mut buf[..to_read], pos).map_err(|e| e.to_string())?;
            writer
                .write_at(&buf[..to_read], pos)
                .map_err(|e| e.to_string())?;
            pos += to_read as u64;
        }
        Ok(())
    }
}
