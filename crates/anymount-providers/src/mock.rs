use anymount_core::{Error, FileMetadata, FileType, Result, StorageProvider};
use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// A mock storage provider that simulates a filesystem in memory.
/// Perfect for testing and demonstrating the mount functionality.
pub struct MockProvider {
    files: Arc<parking_lot::RwLock<HashMap<String, MockFile>>>,
}

#[derive(Clone)]
struct MockFile {
    content: Bytes,
    file_type: FileType,
    modified: u64,
}

impl MockProvider {
    pub fn new() -> Self {
        let files = Arc::new(parking_lot::RwLock::new(HashMap::new()));
        let provider = Self { files };
        provider.setup_mock_filesystem();
        provider
    }

    /// Set up a mock filesystem with some sample files and directories
    fn setup_mock_filesystem(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut files = self.files.write();

        // Root directory
        files.insert(
            "/".to_string(),
            MockFile {
                content: Bytes::new(),
                file_type: FileType::Directory,
                modified: now,
            },
        );

        // Sample files in root
        files.insert(
            "/README.txt".to_string(),
            MockFile {
                content: Bytes::from_static(
                    b"Welcome to anymount!\n\nThis is a mock filesystem for testing.\nExplore the directories below!\n",
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

        // Documents directory
        files.insert(
            "/documents".to_string(),
            MockFile {
                content: Bytes::new(),
                file_type: FileType::Directory,
                modified: now,
            },
        );

        files.insert(
            "/documents/report.txt".to_string(),
            MockFile {
                content: Bytes::from_static(
                    b"Annual Report 2024\n==================\n\nThis is a sample document.\n",
                ),
                file_type: FileType::File,
                modified: now - 3600,
            },
        );

        files.insert(
            "/documents/notes.txt".to_string(),
            MockFile {
                content: Bytes::from_static(b"Meeting notes:\n- Discuss project goals\n- Review timeline\n- Assign tasks\n"),
                file_type: FileType::File,
                modified: now - 7200,
            },
        );

        // Photos directory
        files.insert(
            "/photos".to_string(),
            MockFile {
                content: Bytes::new(),
                file_type: FileType::Directory,
                modified: now,
            },
        );

        files.insert(
            "/photos/vacation.jpg".to_string(),
            MockFile {
                content: Bytes::from_static(b"[Mock JPEG data - not a real image]\n"),
                file_type: FileType::File,
                modified: now - 86400,
            },
        );

        files.insert(
            "/photos/family.jpg".to_string(),
            MockFile {
                content: Bytes::from_static(b"[Mock JPEG data - not a real image]\n"),
                file_type: FileType::File,
                modified: now - 172800,
            },
        );

        // Code directory with subdirectory
        files.insert(
            "/code".to_string(),
            MockFile {
                content: Bytes::new(),
                file_type: FileType::Directory,
                modified: now,
            },
        );

        files.insert(
            "/code/rust".to_string(),
            MockFile {
                content: Bytes::new(),
                file_type: FileType::Directory,
                modified: now,
            },
        );

        files.insert(
            "/code/rust/main.rs".to_string(),
            MockFile {
                content: Bytes::from_static(
                    b"fn main() {\n    println!(\"Hello from mock filesystem!\");\n}\n",
                ),
                file_type: FileType::File,
                modified: now - 1800,
            },
        );

        files.insert(
            "/code/rust/lib.rs".to_string(),
            MockFile {
                content: Bytes::from_static(
                    b"pub fn greet(name: &str) -> String {\n    format!(\"Hello, {}!\", name)\n}\n",
                ),
                file_type: FileType::File,
                modified: now - 1800,
            },
        );

        // Large file for testing
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

    fn get_directory_children(&self, path: &str) -> Vec<String> {
        let files = self.files.read();
        let normalized = self.normalize_path(path);
        let prefix = if normalized == "/" {
            "/".to_string()
        } else {
            format!("{}/", normalized)
        };

        let mut children = std::collections::HashSet::new();

        for key in files.keys() {
            if key == &normalized {
                continue;
            }

            // For root, match files starting with /
            if normalized == "/" {
                if let Some(name) = key.strip_prefix('/') {
                    // Get the first component after /
                    if let Some(first_part) = name.split('/').next() {
                        if !first_part.is_empty() {
                            children.insert(first_part.to_string());
                        }
                    }
                }
            } else {
                // For subdirectories
                if let Some(stripped) = key.strip_prefix(&prefix) {
                    // Only include direct children (not nested)
                    if let Some(child_name) = stripped.split('/').next() {
                        if !child_name.is_empty() {
                            children.insert(child_name.to_string());
                        }
                    }
                }
            }
        }

        children.into_iter().collect()
    }
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StorageProvider for MockProvider {
    fn provider_type(&self) -> &str {
        "mock"
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<FileMetadata>> {
        let normalized = self.normalize_path(path);
        let files = self.files.read();

        // Check if the path exists and is a directory
        match files.get(&normalized) {
            Some(file) if file.file_type == FileType::Directory => {}
            Some(_) => {
                return Err(Error::NotFound(format!(
                    "Path '{}' is not a directory",
                    path
                )))
            }
            None => return Err(Error::NotFound(format!("Path '{}' not found", path))),
        }

        let children = self.get_directory_children(&normalized);
        let mut results = Vec::new();

        for child_name in children {
            let child_path = if normalized == "/" {
                format!("/{}", child_name)
            } else {
                format!("{}/{}", normalized, child_name)
            };

            if let Some(file) = files.get(&child_path) {
                results.push(FileMetadata {
                    path: child_name,
                    file_type: file.file_type,
                    size: file.content.len() as u64,
                    modified: Some(file.modified),
                    created: Some(file.modified),
                });
            }
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
            .unwrap()
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
            .unwrap()
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
                // Check if directory is empty
                let prefix = if normalized == "/" {
                    "".to_string()
                } else {
                    format!("{}/", normalized)
                };

                let has_children = files.keys().any(|k| k != &normalized && k.starts_with(&prefix));

                if has_children {
                    return Err(Error::Provider(format!(
                        "Directory '{}' is not empty",
                        path
                    )));
                }

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
        let provider = MockProvider::new();
        let entries = provider.list_dir("/").await.unwrap();
        
        // Root has several items
        assert!(!entries.is_empty(), "Root directory should have entries");
        
        // Check for expected root items
        let names: Vec<_> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(names.contains(&"README.txt") || names.contains(&"hello.txt"),
                "Root should contain sample files, got: {:?}", names);
    }

    #[tokio::test]
    async fn test_read_file() {
        let provider = MockProvider::new();
        let content = provider.read_file("/hello.txt").await.unwrap();
        assert!(!content.is_empty());
    }

    #[tokio::test]
    async fn test_write_and_read() {
        let provider = MockProvider::new();
        let data = Bytes::from("test content");
        provider.write_file("/test.txt", data.clone()).await.unwrap();
        
        let read_data = provider.read_file("/test.txt").await.unwrap();
        assert_eq!(data, read_data);
    }

    #[tokio::test]
    async fn test_create_directory() {
        let provider = MockProvider::new();
        provider.create_dir("/newdir").await.unwrap();
        
        let metadata = provider.get_metadata("/newdir").await.unwrap();
        assert_eq!(metadata.file_type, FileType::Directory);
    }

    #[tokio::test]
    async fn test_delete_file() {
        let provider = MockProvider::new();
        let data = Bytes::from("temporary");
        provider.write_file("/temp.txt", data).await.unwrap();
        
        provider.delete_file("/temp.txt").await.unwrap();
        
        assert!(!provider.exists("/temp.txt").await.unwrap());
    }
}

