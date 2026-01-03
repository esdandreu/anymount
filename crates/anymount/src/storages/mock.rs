use crate::{Error, Result};
use super::Storage;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// A mock storage provider that simulates a filesystem in memory.
/// Perfect for testing and demonstrating the mount functionality.
pub struct MockStorage {
    files: Arc<parking_lot::RwLock<HashMap<String, MockFile>>>,
}


/// Metadata about a file or directory in the storage provider
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub path: String,
    pub file_type: FileType,
    pub size: u64,
    pub modified: Option<u64>, // Unix timestamp
    pub created: Option<u64>,  // Unix timestamp
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File,
    Directory,
}

#[derive(Clone)]
pub struct MockFile {
    pub content: Bytes,
    pub file_type: FileType,
    pub modified: u64,
}

impl MockStorage {
    pub fn new() -> Self {
        let files = Arc::new(parking_lot::RwLock::new(HashMap::new()));
        let provider = Self { files };
        provider.setup_mock_filesystem();
        provider
    }

    fn setup_mock_filesystem(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut files = self.files.write();

        files.insert(
            "/README.txt".to_string(),
            MockFile {
                content: Bytes::from_static(
                    b"Welcome to anymount!\n\nThis is a mock filesystem for testing.\n",
                ),
                file_type: FileType::File,
                modified: now,
            },
        );

        files.insert(
            "/hello.txt".to_string(),
            MockFile {
                content: Bytes::from_static(b"Hello from anymount mock filesystem!\n"),
                file_type: FileType::File,
                modified: now,
            },
        );

        files.insert(
            "/report.txt".to_string(),
            MockFile {
                content: Bytes::from_static(
                    b"Annual Report 2024\n==================\n\nThis is a sample document.\n",
                ),
                file_type: FileType::File,
                modified: now - 3600,
            },
        );

        files.insert(
            "/notes.txt".to_string(),
            MockFile {
                content: Bytes::from_static(b"Meeting notes:\n- Discuss project goals\n- Review timeline\n- Assign tasks\n"),
                file_type: FileType::File,
                modified: now - 7200,
            },
        );

        files.insert(
            "/vacation.jpg".to_string(),
            MockFile {
                content: Bytes::from_static(b"[Mock JPEG data - not a real image]\n"),
                file_type: FileType::File,
                modified: now - 86400,
            },
        );

        files.insert(
            "/family.jpg".to_string(),
            MockFile {
                content: Bytes::from_static(b"[Mock JPEG data - not a real image]\n"),
                file_type: FileType::File,
                modified: now - 172800,
            },
        );

        files.insert(
            "/main.rs".to_string(),
            MockFile {
                content: Bytes::from_static(
                    b"fn main() {\n    println!(\"Hello from mock filesystem!\");\n}\n",
                ),
                file_type: FileType::File,
                modified: now - 1800,
            },
        );

        files.insert(
            "/lib.rs".to_string(),
            MockFile {
                content: Bytes::from_static(
                    b"pub fn greet(name: &str) -> String {\n    format!(\"Hello, {}!\", name)\n}\n",
                ),
                file_type: FileType::File,
                modified: now - 1800,
            },
        );

        files.insert(
            "/largefile.txt".to_string(),
            MockFile {
                content: Bytes::from(vec![b'A'; 10_000]),
                file_type: FileType::File,
                modified: now,
            },
        );
    }

    fn normalize_path(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", path)
        }
    }

    fn provider_type(&self) -> &str {
        "mock"
    }

    async fn list_dir(&self, _path: &str) -> Result<Vec<FileMetadata>> {
        let files = self.files.read();
        let mut results = Vec::new();

        for (file_path, file) in files.iter() {
            let name = file_path.trim_start_matches('/');
            results.push(FileMetadata {
                path: name.to_string(),
                file_type: file.file_type,
                size: file.content.len() as u64,
                modified: Some(file.modified),
                created: Some(file.modified),
            });
        }

        Ok(results)
    }

    async fn get_metadata(&self, path: &str) -> Result<FileMetadata> {
        let normalized = self.normalize_path(path);
        let files = self.files.read();

        match files.get(&normalized) {
            Some(file) => Ok(FileMetadata {
                path: path.to_string(),
                file_type: file.file_type,
                size: file.content.len() as u64,
                modified: Some(file.modified),
                created: Some(file.modified),
            }),
            None => Err(Error::NotFound(format!("Path '{}' not found", path))),
        }
    }

    async fn read_file(&self, path: &str) -> Result<Bytes> {
        let normalized = self.normalize_path(path);
        let files = self.files.read();

        match files.get(&normalized) {
            Some(file) if file.file_type == FileType::File => Ok(file.content.clone()),
            Some(_) => Err(Error::Provider(format!(
                "Path '{}' is not a file",
                path
            ))),
            None => Err(Error::NotFound(format!("File '{}' not found", path))),
        }
    }

    async fn read_file_range(&self, path: &str, offset: u64, length: u64) -> Result<Bytes> {
        let content = self.read_file(path).await?;
        let start = offset as usize;
        let end = (offset + length).min(content.len() as u64) as usize;

        if start >= content.len() {
            return Ok(Bytes::new());
        }

        Ok(content.slice(start..end))
    }

    async fn write_file(&self, path: &str, data: Bytes) -> Result<()> {
        let normalized = self.normalize_path(path);
        let mut files = self.files.write();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        files.insert(
            normalized,
            MockFile {
                content: data,
                file_type: FileType::File,
                modified: now,
            },
        );

        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        let normalized = self.normalize_path(path);
        let mut files = self.files.write();

        if files.contains_key(&normalized) {
            return Err(Error::Provider(format!(
                "Path '{}' already exists",
                path
            )));
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        files.insert(
            normalized,
            MockFile {
                content: Bytes::new(),
                file_type: FileType::Directory,
                modified: now,
            },
        );

        Ok(())
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        let normalized = self.normalize_path(path);
        let mut files = self.files.write();

        match files.get(&normalized) {
            Some(file) if file.file_type == FileType::File => {
                files.remove(&normalized);
                Ok(())
            }
            Some(_) => Err(Error::Provider(format!(
                "Path '{}' is not a file",
                path
            ))),
            None => Err(Error::NotFound(format!("File '{}' not found", path))),
        }
    }

    async fn delete_dir(&self, path: &str) -> Result<()> {
        let normalized = self.normalize_path(path);
        let mut files = self.files.write();

        match files.get(&normalized) {
            Some(file) if file.file_type == FileType::Directory => {
                files.remove(&normalized);
                Ok(())
            }
            Some(_) => Err(Error::Provider(format!(
                "Path '{}' is not a directory",
                path
            ))),
            None => Err(Error::NotFound(format!(
                "Directory '{}' not found",
                path
            ))),
        }
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        let normalized = self.normalize_path(path);
        let files = self.files.read();
        Ok(files.contains_key(&normalized))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_root() {
        let provider = MockStorage::new();
        let entries = provider.list_dir("/").await.unwrap();
        
        assert!(!entries.is_empty(), "Root directory should have entries");
        
        let names: Vec<_> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(names.contains(&"README.txt") || names.contains(&"hello.txt"),
                "Root should contain sample files, got: {:?}", names);
    }

    #[tokio::test]
    async fn test_read_file() {
        let provider = MockStorage::new();
        let content = provider.read_file("/hello.txt").await.unwrap();
        assert!(!content.is_empty());
    }

    #[tokio::test]
    async fn test_write_and_read() {
        let provider = MockStorage::new();
        let data = Bytes::from("test content");
        provider.write_file("/test.txt", data.clone()).await.unwrap();
        
        let read_data = provider.read_file("/test.txt").await.unwrap();
        assert_eq!(data, read_data);
    }

    #[tokio::test]
    async fn test_create_directory() {
        let provider = MockStorage::new();
        provider.create_dir("/newdir").await.unwrap();
        
        let metadata = provider.get_metadata("/newdir").await.unwrap();
        assert_eq!(metadata.file_type, FileType::Directory);
    }

    #[tokio::test]
    async fn test_delete_file() {
        let provider = MockStorage::new();
        let data = Bytes::from("temporary");
        provider.write_file("/temp.txt", data).await.unwrap();
        
        provider.delete_file("/temp.txt").await.unwrap();
        
        assert!(!provider.exists("/temp.txt").await.unwrap());
    }
}

impl Storage for MockStorage {}

impl Default for MockStorage {
    fn default() -> Self {
        Self::new()
    }
}

