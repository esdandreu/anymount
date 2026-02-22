//! Linux provider: FUSE mount + D-Bus org.freedesktop.CloudProviders.

use super::dbus::{AccountExporter, PROVIDER_PATH, ProviderExporter, request_bus_name};
use super::fuse::StorageFilesystem;
use crate::providers::Provider;
use crate::storages::Storage;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::result::Result;
use tracing::info;

/// Linux provider: FUSE mount backed by Storage + D-Bus CloudProviders advertisement.
pub struct LibCloudProvider {
    path: PathBuf,
    _session: fuser::BackgroundSession,
}

impl LibCloudProvider {
    /// Create a provider from an already-mounted FUSE session (path and session from mount).
    pub fn new(path: PathBuf, session: fuser::BackgroundSession) -> Self {
        Self {
            path,
            _session: session,
        }
    }
}

impl Provider for LibCloudProvider {
    fn kind(&self) -> &'static str {
        "LibCloudProviders"
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

fn default_cache_base_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(path);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache");
    }
    std::env::temp_dir()
}

fn cache_root_for_mount(path: &PathBuf) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    let mount_hash = format!("{:016x}", hasher.finish());
    default_cache_base_dir()
        .join("anymount")
        .join("linux")
        .join(mount_hash)
}

/// Mount storage at path with FUSE and return (path, BackgroundSession).
pub fn mount_storage<S: Storage>(
    path: PathBuf,
    storage: S,
) -> Result<(PathBuf, fuser::BackgroundSession), String> {
    if !path.exists() {
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("Failed to create mount path: {}", e))?;
    }
    let path = path
        .canonicalize()
        .map_err(|e| format!("Mount path: {}", e))?;
    info!("Mount path: {}", path.display());
    let cache_root = cache_root_for_mount(&path);
    std::fs::create_dir_all(&cache_root).map_err(|e| {
        format!(
            "Failed to create cache directory {}: {}",
            cache_root.display(),
            e
        )
    })?;
    info!("Cache path: {}", cache_root.display());

    let fs = StorageFilesystem::new(storage, cache_root)?;
    let session = fuser::spawn_mount2(fs, &path, &fuser::Config::default())
        .map_err(|e| format!("FUSE mount failed: {}", e))?;
    Ok((path, session))
}

/// Register provider and accounts on D-Bus and spawn the connection loop.
pub async fn export_on_dbus(accounts: &[(PathBuf, AccountExporter)]) -> Result<(), String> {
    let connection = zbus::Connection::session()
        .await
        .map_err(|e| format!("D-Bus connection: {}", e))?;
    request_bus_name(&connection)
        .await
        .map_err(|e| format!("D-Bus request name: {}", e))?;

    connection
        .object_server()
        .at(PROVIDER_PATH, ProviderExporter::default())
        .await
        .map_err(|e| format!("D-Bus Provider: {}", e))?;
    connection
        .object_server()
        .at(PROVIDER_PATH, zbus::fdo::ObjectManager)
        .await
        .map_err(|e| format!("D-Bus ObjectManager: {}", e))?;

    for (i, (_path, account)) in accounts.iter().enumerate() {
        let object_path = format!("/org/anymount/CloudProviders/Account_{}", i);
        connection
            .object_server()
            .at(object_path.as_str(), account.clone())
            .await
            .map_err(|e| format!("D-Bus Account {}: {}", i, e))?;
    }

    tokio::spawn(async move {
        let _ = connection;
        std::future::pending::<()>().await;
    });
    Ok(())
}
