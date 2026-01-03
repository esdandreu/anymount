use super::provider::ID_PREFIX;
use crate::providers::{ProviderConfiguration, ProvidersConfiguration};
use tracing::{error, info};
use windows::{
    Foundation::Collections::IVectorView,
    Storage::Provider::{StorageProviderSyncRootInfo, StorageProviderSyncRootManager},
};

/// Cleanup the registry of any non-configured registered sync roots.
pub fn cleanup_registry(configuration: &impl ProvidersConfiguration) -> Result<(), String> {
    _cleanup_registry::<StorageProviderSyncRootManager>(configuration)
}

/// Trait for a registry manager.
trait RegistryManager {
    fn get_currently_registered() -> Result<IVectorView<StorageProviderSyncRootInfo>, String>;
    fn unregister(id: &windows::core::HSTRING) -> Result<(), String>;
}

/// Implementation of the RegistryManager trait for StorageProviderSyncRootManager.
impl RegistryManager for StorageProviderSyncRootManager {
    fn get_currently_registered() -> Result<IVectorView<StorageProviderSyncRootInfo>, String> {
        StorageProviderSyncRootManager::GetCurrentSyncRoots().map_err(|e| e.to_string())
    }
    fn unregister(id: &windows::core::HSTRING) -> Result<(), String> {
        StorageProviderSyncRootManager::Unregister(id).map_err(|e| e.to_string())
    }
}

/// Cleanup the registry of any non-configured registered sync roots.
fn _cleanup_registry<Registry: RegistryManager>(
    configuration: &impl ProvidersConfiguration,
) -> Result<(), String> {
    let sync_roots = Registry::get_currently_registered()?;
    for sync_root in sync_roots {
        let id = match sync_root.Id() {
            Ok(id) => id,
            Err(_) => continue,
        };

        // Skip if not an Anymount sync root
        if !id.to_string().starts_with(ID_PREFIX) {
            continue;
        }

        // Get the path of the sync root
        let sync_root_path = match get_sync_root_path(&sync_root) {
            Ok(path) => path,
            Err(_) => {
                info!(
                    "Failed to get path for sync root {}, skipping",
                    id.to_string()
                );
                continue;
            }
        };

        // Only unregister if not configured
        if is_path_configured(&sync_root_path, configuration) {
            continue;
        }

        match Registry::unregister(&id) {
            Ok(()) => info!(
                "Unregistered non-configured sync root {} at {}",
                id.to_string(),
                sync_root_path
            ),
            Err(e) => error!(
                "Failed to unregister {} at {} {:?}",
                id.to_string(),
                sync_root_path,
                e
            ),
        }
    }

    Ok(())
}

/// Check if a path is part of a configured provider.
fn is_path_configured(path: &str, configuration: &impl ProvidersConfiguration) -> bool {
    for provider in configuration.providers() {
        let provider_path = provider.path().to_string_lossy().to_string();
        if path.eq_ignore_ascii_case(&provider_path) {
            return true;
        }
    }
    return false;
}

/// Get the path of a sync root.
fn get_sync_root_path(sync_root: &StorageProviderSyncRootInfo) -> Result<String, String> {
    let folder = sync_root.Path().map_err(|e| e.to_string())?;
    let path = folder.Path().map_err(|e| e.to_string())?;
    Ok(path.to_string())
}
