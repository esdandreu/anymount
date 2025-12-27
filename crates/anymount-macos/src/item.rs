/*!
NSFileProviderItem implementation for anymount.

This module provides a bridge between anymount's FileMetadata and
Apple's NSFileProviderItem protocol.
*/

use anymount_core::provider::{FileMetadata, FileType};
use objc2::rc::Retained;
use objc2_foundation::NSDate;
use std::path::PathBuf;

/// A Rust-friendly wrapper around NSFileProviderItem
///
/// This struct bridges anymount's FileMetadata with Apple's FileProvider protocol.
/// It allows our storage providers to be exposed through the macOS FileProvider system.
#[derive(Debug, Clone)]
pub struct FileProviderItem {
    metadata: FileMetadata,
    identifier: String,
    parent_identifier: String,
}

impl FileProviderItem {
    /// Create a new FileProviderItem
    ///
    /// # Arguments
    /// * `metadata` - The file metadata from the storage provider
    /// * `identifier` - Unique identifier for this item
    /// * `parent_identifier` - Identifier of the parent directory
    pub fn new(metadata: FileMetadata, identifier: String, parent_identifier: String) -> Self {
        Self {
            metadata,
            identifier,
            parent_identifier,
        }
    }

    /// Get the item identifier
    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    /// Get the parent item identifier
    pub fn parent_identifier(&self) -> &str {
        &self.parent_identifier
    }

    /// Get the filename
    pub fn filename(&self) -> String {
        PathBuf::from(&self.metadata.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.metadata.path)
            .to_string()
    }

    /// Get the underlying metadata
    pub fn metadata(&self) -> &FileMetadata {
        &self.metadata
    }

    /// Get the content type (UTI)
    pub fn content_type(&self) -> String {
        match self.metadata.file_type {
            FileType::Directory => "public.folder".to_string(),
            FileType::File => {
                // Try to determine type from extension
                let path = PathBuf::from(&self.metadata.path);
                match path.extension().and_then(|e| e.to_str()) {
                    Some("txt") => "public.plain-text".to_string(),
                    Some("pdf") => "com.adobe.pdf".to_string(),
                    Some("jpg") | Some("jpeg") => "public.jpeg".to_string(),
                    Some("png") => "public.png".to_string(),
                    Some("mp4") => "public.mpeg-4".to_string(),
                    Some("zip") => "public.zip-archive".to_string(),
                    Some("json") => "public.json".to_string(),
                    _ => "public.data".to_string(),
                }
            }
        }
    }

    /// Get the capabilities for this item
    pub fn capabilities(&self) -> u64 {
        let mut caps = 0u64;

        // All items can be read and deleted
        caps |= 1 << 0; // AllowsReading
        caps |= 1 << 1; // AllowsDeleting
        caps |= 1 << 2; // AllowsRenaming

        // Files can be written to
        if matches!(self.metadata.file_type, FileType::File) {
            caps |= 1 << 3; // AllowsWriting
        }

        // Directories can have content added
        if matches!(self.metadata.file_type, FileType::Directory) {
            caps |= 1 << 4; // AllowsContentEnumerating
            caps |= 1 << 5; // AllowsAddingSubItems
        }

        caps
    }

    /// Convert Unix timestamp to NSDate
    fn timestamp_to_nsdate(timestamp: Option<u64>) -> Option<Retained<NSDate>> {
        timestamp.map(|ts| {
            // Unix timestamp is seconds since 1970
            // NSDate uses seconds since 2001-01-01
            let ns_epoch_offset = 978307200.0; // Seconds between 1970 and 2001
            let ns_time = ts as f64 - ns_epoch_offset;
            NSDate::dateWithTimeIntervalSinceReferenceDate(ns_time)
        })
    }

    /// Create an NSDate for the modification time
    pub fn content_modification_date(&self) -> Option<Retained<NSDate>> {
        Self::timestamp_to_nsdate(self.metadata.modified)
    }

    /// Create an NSDate for the creation time
    pub fn creation_date(&self) -> Option<Retained<NSDate>> {
        Self::timestamp_to_nsdate(self.metadata.created)
    }

    /// Get the document size
    pub fn document_size(&self) -> Option<u64> {
        match self.metadata.file_type {
            FileType::File => Some(self.metadata.size),
            FileType::Directory => None,
        }
    }
}

/// Convert a path to a FileProvider item identifier
///
/// This creates a unique, consistent identifier from a path.
/// The root path "/" maps to NSFileProviderRootContainerItemIdentifier.
pub fn path_to_identifier(path: &str) -> String {
    if path == "/" || path.is_empty() {
        // Use the system-defined root container identifier
        "NSFileProviderRootContainerItemIdentifier".to_string()
    } else {
        // Create a unique identifier based on the path
        // In production, you might want to use a hash or UUID mapping
        format!("item:{}", path.trim_start_matches('/'))
    }
}

/// Convert an identifier back to a path
pub fn identifier_to_path(identifier: &str) -> Option<String> {
    if identifier == "NSFileProviderRootContainerItemIdentifier" {
        Some("/".to_string())
    } else {
        identifier
            .strip_prefix("item:")
            .map(|path| format!("/{}", path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_to_identifier_root() {
        assert_eq!(
            path_to_identifier("/"),
            "NSFileProviderRootContainerItemIdentifier"
        );
        assert_eq!(
            path_to_identifier(""),
            "NSFileProviderRootContainerItemIdentifier"
        );
    }

    #[test]
    fn test_path_to_identifier_regular() {
        assert_eq!(path_to_identifier("/foo/bar.txt"), "item:foo/bar.txt");
        assert_eq!(path_to_identifier("/documents"), "item:documents");
    }

    #[test]
    fn test_identifier_to_path() {
        assert_eq!(
            identifier_to_path("NSFileProviderRootContainerItemIdentifier"),
            Some("/".to_string())
        );
        assert_eq!(
            identifier_to_path("item:foo/bar.txt"),
            Some("/foo/bar.txt".to_string())
        );
        assert_eq!(identifier_to_path("invalid"), None);
    }

    #[test]
    fn test_round_trip() {
        let paths = vec!["/", "/foo", "/foo/bar/baz.txt"];
        for path in paths {
            let id = path_to_identifier(path);
            let recovered = identifier_to_path(&id).unwrap();
            assert_eq!(path, recovered);
        }
    }
}

