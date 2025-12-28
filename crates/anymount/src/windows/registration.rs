use crate::Result;
use std::path::{Path, PathBuf};
use tracing::{error, info, warn};
use windows::Storage::Provider::*;
use windows::Storage::StorageFolder;
use windows::core::HSTRING;
use windows::Win32::Storage::CloudFilters::{
    CF_CONNECTION_KEY, CfConnectSyncRoot, CfDisconnectSyncRoot, CF_CONNECT_FLAGS
};

/// Configuration for registering a Windows sync root
#[derive(Debug, Clone)]
pub struct RegistrationConfig {
    /// Path where files will be synchronized
    pub sync_root_path: PathBuf,
    
    /// Display name shown in File Explorer
    pub display_name: String,
    
    /// Unique provider identifier (GUID format recommended)
    pub provider_id: String,
    
    /// Provider version
    pub provider_version: String,
    
    /// Icon resource path (optional)
    pub icon_resource: Option<String>,
    
    /// Whether to show overlays in File Explorer
    pub show_overlays: bool,

    /// Population policy - whether to auto-populate placeholders
    pub auto_populate: bool,

    /// Hydration policy - when to download file contents
    pub hydration_policy: HydrationPolicy,
}

#[derive(Debug, Clone, Copy)]
pub enum HydrationPolicy {
    /// Download files when first accessed
    Progressive,
    /// Download files fully when opened
    Full,
    /// Always keep files online until explicitly downloaded
    AlwaysFull,
}

impl Default for RegistrationConfig {
    fn default() -> Self {
        Self {
            sync_root_path: PathBuf::from(r"C:\Users\Public\Anymount"),
            display_name: "Anymount".to_string(),
            provider_id: "Anymount.CloudProvider".to_string(),
            provider_version: env!("CARGO_PKG_VERSION").to_string(),
            icon_resource: None,
            show_overlays: true,
            auto_populate: true,
            hydration_policy: HydrationPolicy::Progressive,
        }
    }
}

/// Register a sync root with Windows Cloud Filter API
pub fn register_sync_root(config: &RegistrationConfig) -> Result<()> {
    info!(
        "Registering sync root: {} at {:?}",
        config.display_name,
        config.sync_root_path
    );

    // Ensure the sync root directory exists
    std::fs::create_dir_all(&config.sync_root_path)?;

    // Create StorageProviderSyncRootInfo using WinRT API
    let sync_root_info = StorageProviderSyncRootInfo::new()
        .map_err(|e| crate::Error::Platform(format!("Failed to create sync root info: {:?}", e)))?;
    
    // Set the ID (use folder name as ID)
    let folder_name = config.sync_root_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Anymount");
    sync_root_info.SetId(&HSTRING::from(folder_name))
        .map_err(|e| crate::Error::Platform(format!("Failed to set ID: {:?}", e)))?;
    
    // Set the path using StorageFolder
    let path_hstring = HSTRING::from(config.sync_root_path.to_string_lossy().as_ref());
    let folder = StorageFolder::GetFolderFromPathAsync(&path_hstring)
        .map_err(|e| crate::Error::Platform(format!("Failed to get folder async: {:?}", e)))?
        .get()
        .map_err(|e| crate::Error::Platform(format!("Failed to get folder: {:?}", e)))?;
    sync_root_info.SetPath(&folder)
        .map_err(|e| crate::Error::Platform(format!("Failed to set path: {:?}", e)))?;
    
    // Set display name
    sync_root_info.SetDisplayNameResource(&HSTRING::from(&config.display_name))
        .map_err(|e| crate::Error::Platform(format!("Failed to set display name: {:?}", e)))?;
    
    // Set icon resource (optional)
    if let Some(icon) = &config.icon_resource {
        sync_root_info.SetIconResource(&HSTRING::from(icon))
            .map_err(|e| crate::Error::Platform(format!("Failed to set icon: {:?}", e)))?;
    } else {
        // Use default Windows folder icon
        sync_root_info.SetIconResource(&HSTRING::from("%SystemRoot%\\system32\\shell32.dll,13"))
            .map_err(|e| crate::Error::Platform(format!("Failed to set default icon: {:?}", e)))?;
    }
    
    // Set hydration policy
    let hydration_policy = match config.hydration_policy {
        HydrationPolicy::Progressive => StorageProviderHydrationPolicy::Progressive,
        HydrationPolicy::Full => StorageProviderHydrationPolicy::Full,
        HydrationPolicy::AlwaysFull => StorageProviderHydrationPolicy::AlwaysFull,
    };
    sync_root_info.SetHydrationPolicy(hydration_policy)
        .map_err(|e| crate::Error::Platform(format!("Failed to set hydration policy: {:?}", e)))?;
    sync_root_info.SetHydrationPolicyModifier(StorageProviderHydrationPolicyModifier::None)
        .map_err(|e| crate::Error::Platform(format!("Failed to set hydration policy modifier: {:?}", e)))?;
    
    // Set population policy
    let population_policy = if config.auto_populate {
        StorageProviderPopulationPolicy::Full
    } else {
        StorageProviderPopulationPolicy::AlwaysFull
    };
    sync_root_info.SetPopulationPolicy(population_policy)
        .map_err(|e| crate::Error::Platform(format!("Failed to set population policy: {:?}", e)))?;
    
    // Set other policies
    sync_root_info.SetInSyncPolicy(StorageProviderInSyncPolicy::PreserveInsyncForSyncEngine)
        .map_err(|e| crate::Error::Platform(format!("Failed to set in-sync policy: {:?}", e)))?;
    sync_root_info.SetHardlinkPolicy(StorageProviderHardlinkPolicy::None)
        .map_err(|e| crate::Error::Platform(format!("Failed to set hardlink policy: {:?}", e)))?;
    sync_root_info.SetShowSiblingsAsGroup(false)
        .map_err(|e| crate::Error::Platform(format!("Failed to set show siblings as group: {:?}", e)))?;
    sync_root_info.SetVersion(&HSTRING::from(config.provider_version.as_str()))
        .map_err(|e| crate::Error::Platform(format!("Failed to set version: {:?}", e)))?;
    
    // Register using WinRT API
    match StorageProviderSyncRootManager::Register(&sync_root_info) {
        Ok(_) => {
            info!("Sync root registered successfully");
            Ok(())
        }
        Err(e) => {
            error!("Failed to register sync root: {:?}", e);
            Err(crate::Error::Platform(format!(
                "Failed to register sync root: {:?}",
                e
            )))
        }
    }
}

/// Unregister a sync root
pub fn unregister_sync_root(sync_root_path: &Path) -> Result<()> {
    info!("Unregistering sync root: {:?}", sync_root_path);
    
    // Get the folder name as ID
    let folder_name = sync_root_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Anymount");
    
    match StorageProviderSyncRootManager::Unregister(&HSTRING::from(folder_name)) {
        Ok(_) => {
            info!("Sync root unregistered successfully");
            Ok(())
        }
        Err(e) => {
            let err_str = format!("{:?}", e);
            
            // Check for specific error codes
            if err_str.contains("0x8007017C") {
                // ERROR_CLOUD_FILE_INVALID_REQUEST - sync root is still connected
                error!("Failed to unregister sync root: {:?}", e);
                error!("The sync root appears to have an active connection.");
                error!("Please ensure no applications are using the sync root and try again.");
                error!("You may need to restart your computer to fully release the connection.");
                Err(crate::Error::Platform(format!(
                    "Failed to unregister sync root (active connection detected): {:?}\n\
                     Try closing all File Explorer windows that have the sync root open, \n\
                     or restart your computer to release the connection.",
                    e
                )))
            } else if err_str.contains("0x8007018B") {
                // ERROR_CLOUD_FILE_ACCESS_DENIED - permission issue
                error!("Failed to unregister sync root: {:?}", e);
                error!("Access denied. Make sure you're running as Administrator.");
                Err(crate::Error::Platform(format!(
                    "Failed to unregister sync root (access denied): {:?}\n\
                     Please run as Administrator.",
                    e
                )))
            } else {
                error!("Failed to unregister sync root: {:?}", e);
                Err(crate::Error::Platform(format!(
                    "Failed to unregister sync root: {:?}",
                    e
                )))
            }
        }
    }
}

/// Check if a path is a registered sync root
pub fn is_sync_root_registered(_path: &Path) -> Result<bool> {
    let id = HSTRING::from("Anymount");
    
    match StorageProviderSyncRootManager::GetSyncRootInformationForId(&id) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Connect to an existing sync root to start receiving callbacks
pub fn connect_sync_root(sync_root_path: &Path) -> Result<CF_CONNECTION_KEY> {
    info!("Connecting to sync root: {:?}", sync_root_path);
    
    unsafe {
        let path = HSTRING::from(sync_root_path.to_string_lossy().as_ref());
        
        // Connect to sync root (returns CF_CONNECTION_KEY directly in newer API)
        match CfConnectSyncRoot(
            &path,
            std::ptr::null(),
            None,
            CF_CONNECT_FLAGS(0),
        ) {
            Ok(key) => {
                info!("Connected to sync root successfully");
                Ok(key)
            }
            Err(e) => {
                error!("Failed to connect to sync root: {:?}", e);
                Err(crate::Error::Platform(format!(
                    "Failed to connect to sync root: {:?}",
                    e
                )))
            }
        }
    }
}

/// Disconnect from a sync root
pub fn disconnect_sync_root(connection_key: CF_CONNECTION_KEY) -> Result<()> {
    info!("Disconnecting from sync root");
    
    unsafe {
        match CfDisconnectSyncRoot(connection_key) {
            Ok(_) => {
                info!("Disconnected successfully");
                Ok(())
            }
            Err(e) => {
                warn!("Failed to disconnect: {:?}", e);
                Ok(()) // Don't fail on disconnect errors
            }
        }
    }
}
