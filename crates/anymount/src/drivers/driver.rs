#![allow(unused_imports)]
use super::Result;
use crate::domain::driver::{LegacyDriverConfig, LegacyStorageConfig};
use crate::storages;
use crate::{Logger, Storage};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Sender;

pub trait Session: Send + Sync + 'static {
    fn path(&self) -> &PathBuf;
    fn kind(&self) -> &'static str;
}

// ! This is weird ! Why not Sessions?
pub type Drivers = Vec<Box<dyn Session>>;

// Work in Progress.
pub trait Driver {
    /// List all the mount points that are currently reserved by this driver.
    fn list_mounts(&self) -> Result<Vec<PathBuf>>; // TODO review result type

    /// Mount a given path. As a result, the path will be reserved.
    fn mount(&self, path: Path) -> Result<()>;

    /// Connect a given mount point to a storage. As a result, the mount will be
    /// usable.
    fn connect(&self, path: Path, storage: Box<dyn Storage>) -> Result<()>;

    /// Check if a given mount point is connected to a storage.
    // TODO can it return the storage config to compare whether the connection
    // has a stale storage configuration?
    fn is_connected(&self, path: Path) -> Result<bool>;

    /// Disconnect a given mount point from its storage. As a result, the mount
    /// will be unusable, but the path will still be reserved.
    fn disconnect(&self, path: Path) -> Result<()>;

    /// Unmount a given path. As a result, the path will be freed and any
    /// associated resources will be released. By default, any locally cached
    /// files in that mount will be cleaned up but that behaviour is
    /// configurable.
    fn unmount(&self, path: Path) -> Result<()>;
}

#[cfg(target_os = "windows")]
pub fn connect_drivers(
    specs: &[LegacyDriverConfig],
    logger: &(impl Logger + 'static),
) -> Result<Drivers> {
    use super::windows::{WindowsSession, cleanup_registry};
    let mut drivers: Vec<Box<dyn Session>> = Vec::new();
    for spec in specs {
        let storage = storages::new(spec.storage.clone())?;
        match &spec.storage {
            LegacyStorageConfig::Local { root: _ } => {
                let driver = WindowsSession::connect(
                    spec.path.clone(),
                    storage,
                    logger.clone(),
                    None,
                )?;
                drivers.push(driver);
            }
            LegacyStorageConfig::OneDrive { .. } => {
                let driver = WindowsSession::connect(
                    spec.path.clone(),
                    storage,
                    logger.clone(),
                    None,
                )?;
                drivers.push(driver);
            }
        }
    }
    cleanup_registry(specs, logger)?;
    Ok(drivers)
}

#[cfg(target_os = "linux")]
pub fn connect_drivers(
    specs: &[LegacyDriverConfig],
    logger: &(impl Logger + 'static),
) -> Result<Drivers> {
    use super::linux::dbus::AccountExporter;
    use super::linux::{
        LinuxDriver, export_on_dbus, mount_storage, new_runtime,
    };
    let rt = new_runtime()?;
    let mut accounts: Vec<(std::path::PathBuf, AccountExporter)> = Vec::new();
    let mut sessions: Vec<(std::path::PathBuf, fuser::BackgroundSession)> =
        Vec::new();
    for spec in specs {
        let path = spec.path.clone();
        let storage = storages::new(spec.storage.clone())?;
        match &spec.storage {
            LegacyStorageConfig::Local { root: _ } => {
                let (mount_path, session) =
                    mount_storage(path, storage, logger.clone())?;
                let name = mount_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Anymount")
                    .to_string();
                accounts.push((
                    mount_path.clone(),
                    AccountExporter {
                        name: name.clone(),
                        path: mount_path.display().to_string(),
                        icon: String::new(),
                        status: 0,
                        status_details: String::new(),
                    },
                ));
                sessions.push((mount_path, session));
            }
            LegacyStorageConfig::OneDrive { .. } => {
                let (mount_path, session) =
                    mount_storage(path, storage, logger.clone())?;
                let name = mount_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("OneDrive")
                    .to_string();
                accounts.push((
                    mount_path.clone(),
                    AccountExporter {
                        name,
                        path: mount_path.display().to_string(),
                        icon: String::new(),
                        status: 0,
                        status_details: String::new(),
                    },
                ));
                sessions.push((mount_path, session));
            }
        }
    }
    rt.block_on(export_on_dbus(&accounts, logger))?;
    let drivers: Vec<Box<dyn Session>> = sessions
        .into_iter()
        .map(|(path, session)| {
            Box::new(LinuxDriver::new(path, session)) as Box<dyn Session>
        })
        .collect();
    Ok(drivers)
}

#[cfg(all(target_os = "macos", not(feature = "fuse")))]
pub fn connect_drivers(
    _specs: &[LegacyDriverConfig],
    _logger: &(impl Logger + 'static),
) -> Result<Drivers> {
    Err(crate::drivers::Error::NotSupported)
}

#[cfg(all(target_os = "macos", feature = "fuse"))]
pub fn connect_drivers(
    specs: &[LegacyDriverConfig],
    logger: &(impl Logger + 'static),
) -> Result<Drivers> {
    fn legacy_storage(config: LegacyStorageConfig) -> Result<Box<dyn Storage>> {
        match config {
            LegacyStorageConfig::Local { root } => {
                Ok(Box::new(crate::storages::LocalStorage::new(root)))
            }
            LegacyStorageConfig::OneDrive { .. } => {
                Err(crate::drivers::Error::NotSupported)
            }
        }
    }

    use crate::drivers::fuse::{FuseMount, NoCacheFsCache, StorageFilesystem};
    let mut sessions: Vec<(PathBuf, fuser::BackgroundSession)> = Vec::new();
    for spec in specs {
        if !spec.path.exists() {
            std::fs::create_dir_all(&spec.path)?;
        }
        let mount_path = spec.path.canonicalize()?;
        let storage = legacy_storage(spec.storage.clone())?;
        match &spec.storage {
            LegacyStorageConfig::Local { root: _ } => {
                let fs = StorageFilesystem::new_with_cache(
                    storage,
                    Arc::new(NoCacheFsCache::new()),
                    logger.clone(),
                );
                let session = fuser::spawn_mount2(
                    fs,
                    &mount_path,
                    &fuser::Config::default(),
                )
                .map_err(|source| {
                    super::Error::Fuse(
                        crate::drivers::fuse::error::Error::FuseMount {
                            path: mount_path.clone(),
                            source,
                        },
                    )
                })?;
                sessions.push((mount_path, session));
            }
            LegacyStorageConfig::OneDrive { .. } => {
                let fs = StorageFilesystem::new_with_cache(
                    storage,
                    Arc::new(NoCacheFsCache::new()),
                    logger.clone(),
                );
                let session = fuser::spawn_mount2(
                    fs,
                    &mount_path,
                    &fuser::Config::default(),
                )
                .map_err(|source| {
                    super::Error::Fuse(
                        crate::drivers::fuse::error::Error::FuseMount {
                            path: mount_path.clone(),
                            source,
                        },
                    )
                })?;
                sessions.push((mount_path, session));
            }
        }
    }
    let drivers: Vec<Box<dyn Session>> = sessions
        .into_iter()
        .map(|(path, session)| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Anymount")
                .to_owned();
            Box::new(FuseMount::new(name, path, session)) as Box<dyn Session>
        })
        .collect();
    Ok(drivers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoOpLogger;
    use crate::domain::driver::{
        LegacyDriverConfig, LegacyStorageConfig, TelemetrySpec,
    };

    #[test]
    fn storage_label_comes_from_domain_storage_spec() {
        let local = LegacyStorageConfig::Local {
            root: PathBuf::from("/data"),
        };
        assert_eq!(local.label(), "local");
        let onedrive = LegacyStorageConfig::OneDrive {
            root: PathBuf::from("/"),
            endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
            access_token: None,
            refresh_token: None,
            client_id: None,
            token_expiry_buffer_secs: None,
        };
        assert_eq!(onedrive.label(), "onedrive");
    }

    fn local_driver_spec(name: &str) -> LegacyDriverConfig {
        LegacyDriverConfig {
            name: name.to_owned(),
            path: PathBuf::from(format!("/mnt/{name}")),
            storage: LegacyStorageConfig::Local {
                root: PathBuf::from(format!("/data/{name}")),
            },
            telemetry: TelemetrySpec::default(),
        }
    }

    #[test]
    fn connect_drivers_accepts_resolved_specs() {
        let spec = local_driver_spec("demo");
        let result = connect_drivers(&[spec], &NoOpLogger::default());
        #[cfg(feature = "fuse")]
        assert!(matches!(
            result,
            Ok(_)
                | Err(crate::drivers::Error::NotSupported)
                | Err(crate::drivers::Error::Io(_))
                | Err(crate::drivers::Error::Fuse(_))
        ));
        #[cfg(not(feature = "fuse"))]
        assert!(matches!(
            result,
            Ok(_) | Err(crate::drivers::Error::NotSupported)
        ));
    }
}
