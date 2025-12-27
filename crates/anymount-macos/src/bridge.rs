/*!
FileProvider extension bridge.

This module provides the interface for communicating with a FileProvider extension.
Note: A full implementation requires a separate system extension (written in Swift/ObjC)
that implements NSFileProviderReplicatedExtension.

This Rust code provides:
1. The management/control plane for domains
2. The interface for signaling changes
3. Helpers for item management

The actual file serving is done by a FileProvider extension that you'll need to create.
*/

use anymount_core::{provider::StorageProvider, Result};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{
    domain::{DomainInfo, FileProviderDomain},
    item::{path_to_identifier, FileProviderItem},
};

/// Bridge between anymount's StorageProvider and macOS FileProvider
///
/// This struct manages the lifecycle of a FileProvider domain and coordinates
/// between the Rust storage provider and the native FileProvider extension.
pub struct FileProviderBridge {
    domain: FileProviderDomain,
    provider: Arc<dyn StorageProvider>,
    item_cache: Arc<RwLock<ItemCache>>,
}

impl FileProviderBridge {
    /// Create a new FileProvider bridge
    ///
    /// # Arguments
    /// * `provider` - The storage provider to expose through FileProvider
    /// * `domain_id` - Unique identifier for the domain (e.g., "com.anymount.my-bucket")
    /// * `display_name` - Display name shown in Finder (e.g., "My Bucket")
    pub fn new(
        provider: Arc<dyn StorageProvider>,
        domain_id: &str,
        display_name: &str,
    ) -> Result<Self> {
        let domain = FileProviderDomain::new(domain_id, display_name)?;

        Ok(Self {
            domain,
            provider,
            item_cache: Arc::new(RwLock::new(ItemCache::new())),
        })
    }

    /// Mount the storage provider as a FileProvider domain
    ///
    /// This registers the domain with the system, making it visible in Finder.
    /// Note: This requires a FileProvider extension to be installed and properly configured.
    pub async fn mount(&self) -> Result<()> {
        // Register the domain with the system
        self.domain.register().await?;

        tracing::info!(
            "Mounted FileProvider domain: {} ({})",
            self.domain.display_name(),
            self.domain.identifier()
        );

        Ok(())
    }

    /// Unmount the storage provider
    ///
    /// This removes the domain from the system.
    pub async fn unmount(&self) -> Result<()> {
        self.domain.remove().await?;

        tracing::info!(
            "Unmounted FileProvider domain: {}",
            self.domain.identifier()
        );

        Ok(())
    }

    /// Get the domain identifier
    pub fn domain_id(&self) -> String {
        self.domain.identifier()
    }

    /// Get the display name
    pub fn display_name(&self) -> String {
        self.domain.display_name()
    }

    /// Notify the system that an item has changed
    ///
    /// Call this when the underlying storage changes (e.g., file modified, deleted)
    /// to trigger re-enumeration by the FileProvider system.
    pub async fn signal_item_changed(&self, path: &str) -> Result<()> {
        let identifier = path_to_identifier(path);
        self.domain.signal_item_changed(&identifier).await
    }

    /// Notify the system that a directory's contents have changed
    pub async fn signal_directory_changed(&self, dir_path: &str) -> Result<()> {
        self.signal_item_changed(dir_path).await
    }

    /// List items in a directory
    ///
    /// This is used by the FileProvider extension to enumerate contents.
    /// In a full implementation, this would be called via XPC from the extension.
    pub async fn list_items(&self, path: &str) -> Result<Vec<FileProviderItem>> {
        let metadata_list = self.provider.list_dir(path).await?;

        let items: Vec<FileProviderItem> = metadata_list
            .into_iter()
            .map(|metadata| {
                let item_path = metadata.path.clone();
                let identifier = path_to_identifier(&item_path);
                let parent_identifier = path_to_identifier(path);

                FileProviderItem::new(metadata, identifier, parent_identifier)
            })
            .collect();

        // Update cache
        let mut cache = self.item_cache.write().await;
        for item in &items {
            cache.insert(item.identifier().to_string(), item.clone());
        }

        Ok(items)
    }

    /// Get a specific item by path
    pub async fn get_item(&self, path: &str) -> Result<FileProviderItem> {
        let identifier = path_to_identifier(path);

        // Check cache first
        {
            let cache = self.item_cache.read().await;
            if let Some(item) = cache.get(&identifier) {
                return Ok(item.clone());
            }
        }

        // Fetch from provider
        let metadata = self.provider.get_metadata(path).await?;
        let parent_path = std::path::Path::new(path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("/");
        let parent_identifier = path_to_identifier(parent_path);

        let item = FileProviderItem::new(metadata, identifier.clone(), parent_identifier);

        // Update cache
        let mut cache = self.item_cache.write().await;
        cache.insert(identifier, item.clone());

        Ok(item)
    }

    /// Get the storage provider
    pub fn provider(&self) -> &Arc<dyn StorageProvider> {
        &self.provider
    }

    /// List all currently registered domains
    pub async fn list_all_domains() -> Result<Vec<DomainInfo>> {
        FileProviderDomain::get_all_domains().await
    }
}

/// Cache for FileProviderItems
///
/// This reduces redundant API calls to the storage provider.
struct ItemCache {
    items: std::collections::HashMap<String, FileProviderItem>,
}

impl ItemCache {
    fn new() -> Self {
        Self {
            items: std::collections::HashMap::new(),
        }
    }

    fn insert(&mut self, identifier: String, item: FileProviderItem) {
        self.items.insert(identifier, item);
    }

    fn get(&self, identifier: &str) -> Option<&FileProviderItem> {
        self.items.get(identifier)
    }

    #[allow(dead_code)]
    fn remove(&mut self, identifier: &str) {
        self.items.remove(identifier);
    }

    #[allow(dead_code)]
    fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache() {
        use anymount_core::provider::{FileMetadata, FileType};

        let mut cache = ItemCache::new();

        let metadata = FileMetadata {
            path: "/test.txt".to_string(),
            file_type: FileType::File,
            size: 100,
            modified: None,
            created: None,
        };

        let item = FileProviderItem::new(
            metadata,
            "item:test.txt".to_string(),
            "root".to_string(),
        );

        cache.insert("item:test.txt".to_string(), item.clone());

        assert!(cache.get("item:test.txt").is_some());
        assert!(cache.get("nonexistent").is_none());
    }
}

