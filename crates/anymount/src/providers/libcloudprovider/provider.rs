//! Linux provider: FUSE mount + D-Bus org.freedesktop.CloudProviders.

use super::dbus::{
    AccountExporter, ActionMessage, PROVIDER_PATH, ProviderExporter, new_account_interfaces,
    request_bus_name,
};
use super::fuse::StorageFilesystem;
use super::gtk_dbus::{ACTION_FREE_LOCAL_CACHE, ACTION_OPEN_FOLDER};
use crate::Logger;
use crate::providers::Provider;
use crate::storages::Storage;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::result::Result;

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

pub(crate) fn cache_root_for_mount(path: &PathBuf) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    let mount_hash = format!("{:016x}", hasher.finish());
    default_cache_base_dir()
        .join("anymount")
        .join("linux")
        .join(mount_hash)
}

/// Mount storage at path with FUSE and return (path, BackgroundSession).
pub fn mount_storage<S: Storage, L: Logger + Clone + 'static>(
    path: PathBuf,
    storage: S,
    logger: &L,
) -> Result<(PathBuf, fuser::BackgroundSession), String> {
    if !path.exists() {
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("Failed to create mount path: {}", e))?;
    }
    let path = path
        .canonicalize()
        .map_err(|e| format!("Mount path: {}", e))?;
    logger.info(format!("Mount path: {}", path.display()));
    let cache_root = cache_root_for_mount(&path);
    std::fs::create_dir_all(&cache_root).map_err(|e| {
        format!(
            "Failed to create cache directory {}: {}",
            cache_root.display(),
            e
        )
    })?;
    logger.info(format!("Cache path: {}", cache_root.display()));

    let fs = StorageFilesystem::new(storage, cache_root, logger)?;
    let session = fuser::spawn_mount2(fs, &path, &fuser::Config::default())
        .map_err(|e| format!("FUSE mount failed: {}", e))?;
    Ok((path, session))
}

async fn run_actions<L: Logger>(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<ActionMessage>,
    _logger: L,
) {
    while let Some((mount_path, cache_root, action_name)) = rx.recv().await {
        match action_name.as_str() {
            ACTION_OPEN_FOLDER => {
                let _ = open::that(&mount_path);
            }
            ACTION_FREE_LOCAL_CACHE => {
                let cache_root = cache_root.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let _ = std::fs::remove_dir_all(&cache_root);
                    std::fs::create_dir_all(&cache_root).ok();
                })
                .await;
            }
            _ => {}
        }
    }
}

/// Register provider and accounts on D-Bus and spawn the connection loop.
pub async fn export_on_dbus<L: Logger + Clone + 'static>(
    accounts: &[(PathBuf, AccountExporter)],
    logger: &L,
) -> Result<(), String> {
    let connection = zbus::Connection::session()
        .await
        .map_err(|e| format!("D-Bus connection: {}", e))?;
    request_bus_name(&connection)
        .await
        .map_err(|e| format!("D-Bus request name: {}", e))?;

    let (action_tx, action_rx) = tokio::sync::mpsc::unbounded_channel::<ActionMessage>();
    tokio::spawn(run_actions(action_rx, logger.clone()));

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

    for (i, (path, account)) in accounts.iter().enumerate() {
        let cache_root = cache_root_for_mount(path);
        let (cloud, actions, menus) = new_account_interfaces(
            account.clone(),
            path.display().to_string(),
            cache_root,
            action_tx.clone(),
        );
        let object_path = format!("/org/anymount/CloudProviders/Account_{}", i);
        connection
            .object_server()
            .at(object_path.as_str(), cloud)
            .await
            .map_err(|e| format!("D-Bus Account {}: {}", i, e))?;
        connection
            .object_server()
            .at(object_path.as_str(), actions)
            .await
            .map_err(|e| format!("D-Bus Account {} gtk.Actions: {}", i, e))?;
        connection
            .object_server()
            .at(object_path.as_str(), menus)
            .await
            .map_err(|e| format!("D-Bus Account {} gtk.Menus: {}", i, e))?;
    }

    tokio::spawn(async move {
        let _ = connection;
        std::future::pending::<()>().await;
    });
    Ok(())
}
