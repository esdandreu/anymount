//! Linux driver: FUSE mount + D-Bus org.freedesktop.CloudProviders.

use super::dbus::{
    AccountExporter, ActionMessage, PROVIDER_PATH, ProviderExporter, new_account_interfaces,
    request_bus_name,
};
use super::gtk_dbus::{ACTION_FREE_LOCAL_CACHE, ACTION_OPEN_FOLDER};
use super::{Error, Result, StorageFilesystem};
use crate::drivers::Session;
use crate::Logger;
use crate::storages::Storage;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

pub struct LinuxDriver {
    path: PathBuf,
    _session: fuser::BackgroundSession,
}

impl LinuxDriver {
    pub fn new(path: PathBuf, session: fuser::BackgroundSession) -> Self {
        Self {
            path,
            _session: session,
        }
    }
}

impl Session for LinuxDriver {
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

pub fn mount_storage(
    path: PathBuf,
    storage: impl Storage,
    logger: impl Logger + 'static,
) -> Result<(PathBuf, fuser::BackgroundSession)> {
    if !path.exists() {
        std::fs::create_dir_all(&path).map_err(|source| Error::MountIo {
            operation: "create mount path",
            path: path.clone(),
            source,
        })?;
    }
    let path = path.canonicalize().map_err(|source| Error::MountIo {
        operation: "canonicalize mount path",
        path: path.clone(),
        source,
    })?;
    logger.info(format!("Mount path: {}", path.display()));
    let cache_root = cache_root_for_mount(&path);
    logger.info(format!("Cache path: {}", cache_root.display()));

    let fs = StorageFilesystem::new(storage, cache_root, logger.clone())?;
    let session = fuser::spawn_mount2(fs, &path, &fuser::Config::default()).map_err(|source| {
        Error::FuseMount {
            path: path.clone(),
            source,
        }
    })?;
    Ok((path, session))
}

pub fn new_runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Runtime::new().map_err(|source| Error::RuntimeInit { source })
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

pub async fn export_on_dbus<L: Logger + Clone + 'static>(
    accounts: &[(PathBuf, AccountExporter)],
    logger: &L,
) -> Result<()> {
    let connection = zbus::Connection::session()
        .await
        .map_err(|source| Error::Dbus {
            operation: "open session bus",
            source,
        })?;
    request_bus_name(&connection)
        .await
        .map_err(|source| Error::Dbus {
            operation: "request bus name",
            source,
        })?;

    let (action_tx, action_rx) = tokio::sync::mpsc::unbounded_channel::<ActionMessage>();
    tokio::spawn(run_actions(action_rx, logger.clone()));

    connection
        .object_server()
        .at(PROVIDER_PATH, ProviderExporter::default())
        .await
        .map_err(|source| Error::DbusObject {
            operation: "register driver interface",
            object_path: PROVIDER_PATH.to_string(),
            source,
        })?;
    connection
        .object_server()
        .at(PROVIDER_PATH, zbus::fdo::ObjectManager)
        .await
        .map_err(|source| Error::DbusObject {
            operation: "register object manager",
            object_path: PROVIDER_PATH.to_string(),
            source,
        })?;

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
            .map_err(|source| Error::DbusObject {
                operation: "register cloud account interface",
                object_path: object_path.clone(),
                source,
            })?;
        connection
            .object_server()
            .at(object_path.as_str(), actions)
            .await
            .map_err(|source| Error::DbusObject {
                operation: "register gtk actions interface",
                object_path: object_path.clone(),
                source,
            })?;
        connection
            .object_server()
            .at(object_path.as_str(), menus)
            .await
            .map_err(|source| Error::DbusObject {
                operation: "register gtk menus interface",
                object_path: object_path.clone(),
                source,
            })?;
    }

    tokio::spawn(async move {
        let _ = connection;
        std::future::pending::<()>().await;
    });
    Ok(())
}
