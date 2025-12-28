use crate::StorageProvider;
use cloud_filter::placeholder_file::{PlaceholderFile, BatchCreate};
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, error, info};

/// Manually populate the root directory with placeholder files
pub async fn populate_root_directory(
    sync_root: &Path,
    provider: Arc<dyn StorageProvider>,
) -> crate::Result<()> {
    info!("Populating root directory with placeholders...");

    let entries = provider.list_dir("/").await?;
    debug!("Found {} entries to create", entries.len());

    let mut placeholder_files = Vec::new();
    
    for entry in entries {
        debug!("Creating placeholder for: {}", entry.path);
        
        let mut placeholder = PlaceholderFile::new(&entry.path);
        
        if entry.file_type == crate::FileType::File {
            placeholder = placeholder.metadata(
                cloud_filter::metadata::Metadata::default().size(entry.size)
            );
        }
        
        placeholder_files.push(placeholder);
    }

    match placeholder_files.as_mut_slice().create(sync_root) {
        Ok(_) => {
            info!("✓ Created {} placeholder files in root", placeholder_files.len());
            Ok(())
        }
        Err(e) => {
            error!("Failed to create placeholders: {:?}", e);
            Err(crate::Error::Platform(format!(
                "Failed to create placeholders: {:?}",
                e
            )))
        }
    }
}

