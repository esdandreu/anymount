use super::Result;
use crate::domain::driver::{DriverConfig, StorageConfig};
use crate::service::control::messages::ServiceMessage;
use crate::storages::{LocalStorage, OneDriveConfig};
use crate::Logger;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Arc;

pub trait Driver {
    fn kind(&self) -> &'static str;
    fn path(&self) -> &PathBuf;
}

#[cfg(target_os = "windows")]
pub fn connect_drivers(
    specs: &[DriverConfig],
    logger: &(impl Logger + 'static),
) -> Result<Vec<Box<dyn Driver>>> {
    connect_drivers_with_telemetry(specs, logger, None)
}

#[cfg(target_os = "windows")]
pub fn connect_drivers_with_telemetry(
    specs: &[DriverConfig],
    logger: &(impl Logger + 'static),
    service_tx: Option<Sender<ServiceMessage>>,
) -> Result<Vec<Box<dyn Driver>>> {
    use super::windows::{cleanup_registry, WindowsDriver};
    let mut drivers: Vec<Box<dyn Driver>> = Vec::new();
    for spec in specs {
        match &spec.storage {
            StorageConfig::Local { root } => {
                let storage = LocalStorage::new(root.clone());
                let driver = WindowsDriver::connect(
                    spec.path.clone(),
                    storage,
                    logger.clone(),
                    service_tx.clone(),
                )?;
                drivers.push(Box::new(driver) as Box<dyn Driver>);
            }
            StorageConfig::OneDrive {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            } => {
                let config = OneDriveConfig {
                    root: root.clone(),
                    endpoint: endpoint.clone(),
                    access_token: access_token.clone(),
                    refresh_token: refresh_token.clone(),
                    client_id: client_id.clone(),
                    token_expiry_buffer_secs: *token_expiry_buffer_secs,
                };
                let storage = config.connect()?;
                let driver = WindowsDriver::connect(
                    spec.path.clone(),
                    storage,
                    logger.clone(),
                    service_tx.clone(),
                )?;
                drivers.push(Box::new(driver) as Box<dyn Driver>);
            }
        }
    }
    cleanup_registry(specs, logger)?;
    Ok(drivers)
}

#[cfg(target_os = "linux")]
pub fn connect_drivers(
    specs: &[DriverConfig],
    logger: &(impl Logger + 'static),
) -> Result<Vec<Box<dyn Driver>>> {
    connect_drivers_with_telemetry(specs, logger, None)
}

#[cfg(target_os = "linux")]
pub fn connect_drivers_with_telemetry(
    specs: &[DriverConfig],
    logger: &(impl Logger + 'static),
    _service_tx: Option<Sender<ServiceMessage>>,
) -> Result<Vec<Box<dyn Driver>>> {
    use super::linux::dbus::AccountExporter;
    use super::linux::{export_on_dbus, mount_storage, new_runtime, LinuxDriver};
    let rt = new_runtime()?;
    let mut accounts: Vec<(std::path::PathBuf, AccountExporter)> = Vec::new();
    let mut sessions: Vec<(std::path::PathBuf, fuser::BackgroundSession)> = Vec::new();
    for spec in specs {
        let path = spec.path.clone();
        match &spec.storage {
            StorageConfig::Local { root } => {
                let storage = LocalStorage::new(root.clone());
                let (mount_path, session) = mount_storage(path, storage, logger.clone())?;
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
            StorageConfig::OneDrive {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            } => {
                let one_drive_config = OneDriveConfig {
                    root: root.clone(),
                    endpoint: endpoint.clone(),
                    access_token: access_token.clone(),
                    refresh_token: refresh_token.clone(),
                    client_id: client_id.clone(),
                    token_expiry_buffer_secs: *token_expiry_buffer_secs,
                };
                let storage = one_drive_config.connect()?;
                let (mount_path, session) = mount_storage(path, storage, logger.clone())?;
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
    let drivers: Vec<Box<dyn Driver>> = sessions
        .into_iter()
        .map(|(path, session)| Box::new(LinuxDriver::new(path, session)) as Box<dyn Driver>)
        .collect();
    Ok(drivers)
}

#[cfg(all(any(target_os = "linux", target_os = "macos"), not(feature = "fuse")))]
pub fn connect_drivers(
    _specs: &[DriverConfig],
    _logger: &(impl Logger + 'static),
) -> Result<Vec<Box<dyn Driver>>> {
    Err(crate::drivers::Error::NotSupported)
}

#[cfg(all(any(target_os = "linux", target_os = "macos"), not(feature = "fuse")))]
pub fn connect_drivers_with_telemetry(
    _specs: &[DriverConfig],
    _logger: &(impl Logger + 'static),
    _service_tx: Option<Sender<ServiceMessage>>,
) -> Result<Vec<Box<dyn Driver>>> {
    Err(crate::drivers::Error::NotSupported)
}

#[cfg(feature = "fuse")]
pub fn connect_drivers(
    specs: &[DriverConfig],
    logger: &(impl Logger + 'static),
) -> Result<Vec<Box<dyn Driver>>> {
    connect_drivers_with_telemetry(specs, logger, None)
}

#[cfg(feature = "fuse")]
pub fn connect_drivers_with_telemetry(
    specs: &[DriverConfig],
    logger: &(impl Logger + 'static),
    _service_tx: Option<Sender<ServiceMessage>>,
) -> Result<Vec<Box<dyn Driver>>> {
    use crate::drivers::fuse::{NoCacheFsCache, StorageFilesystem};
    let mut sessions: Vec<(PathBuf, fuser::BackgroundSession)> = Vec::new();
    for spec in specs {
        if !spec.path.exists() {
            std::fs::create_dir_all(&spec.path)?;
        }
        let mount_path = spec.path.canonicalize()?;
        match &spec.storage {
            StorageConfig::Local { root } => {
                let storage = LocalStorage::new(root.clone());
                let fs = StorageFilesystem::new_with_cache(
                    storage,
                    Arc::new(NoCacheFsCache::new()),
                    logger.clone(),
                );
                let session = fuser::spawn_mount2(fs, &mount_path, &fuser::Config::default())
                    .map_err(|source| {
                        super::Error::Fuse(crate::drivers::fuse::error::Error::FuseMount {
                            path: mount_path.clone(),
                            source,
                        })
                    })?;
                sessions.push((mount_path, session));
            }
            StorageConfig::OneDrive {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            } => {
                let config = OneDriveConfig {
                    root: root.clone(),
                    endpoint: endpoint.clone(),
                    access_token: access_token.clone(),
                    refresh_token: refresh_token.clone(),
                    client_id: client_id.clone(),
                    token_expiry_buffer_secs: *token_expiry_buffer_secs,
                };
                let storage = config.connect()?;
                let fs = StorageFilesystem::new_with_cache(
                    storage,
                    Arc::new(NoCacheFsCache::new()),
                    logger.clone(),
                );
                let session = fuser::spawn_mount2(fs, &mount_path, &fuser::Config::default())
                    .map_err(|source| {
                        super::Error::Fuse(crate::drivers::fuse::error::Error::FuseMount {
                            path: mount_path.clone(),
                            source,
                        })
                    })?;
                sessions.push((mount_path, session));
            }
        }
    }
    let drivers: Vec<Box<dyn Driver>> = sessions
        .into_iter()
        .map(|(path, session)| Box::new(FuseDriver::new(path, session)) as Box<dyn Driver>)
        .collect();
    Ok(drivers)
}

#[cfg(feature = "fuse")]
pub struct FuseDriver {
    path: PathBuf,
    _session: fuser::BackgroundSession,
}

#[cfg(feature = "fuse")]
impl FuseDriver {
    pub fn new(path: PathBuf, session: fuser::BackgroundSession) -> Self {
        Self {
            path,
            _session: session,
        }
    }
}

#[cfg(feature = "fuse")]
impl Driver for FuseDriver {
    fn kind(&self) -> &'static str {
        "macos"
    }
    fn path(&self) -> &PathBuf {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::driver::{DriverConfig, StorageConfig, TelemetrySpec};
    use crate::NoOpLogger;

    #[test]
    fn storage_label_comes_from_domain_storage_spec() {
        let local = StorageConfig::Local {
            root: PathBuf::from("/data"),
        };
        assert_eq!(local.label(), "local");
        let onedrive = StorageConfig::OneDrive {
            root: PathBuf::from("/"),
            endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
            access_token: None,
            refresh_token: None,
            client_id: None,
            token_expiry_buffer_secs: None,
        };
        assert_eq!(onedrive.label(), "onedrive");
    }

    fn local_driver_spec(name: &str) -> DriverConfig {
        DriverConfig {
            name: name.to_owned(),
            path: PathBuf::from(format!("/mnt/{name}")),
            storage: StorageConfig::Local {
                root: PathBuf::from(format!("/data/{name}")),
            },
            telemetry: TelemetrySpec::default(),
        }
    }

    #[test]
    fn connect_drivers_accepts_resolved_specs() {
        let spec = local_driver_spec("demo");
        let result = connect_drivers(&[spec], &NoOpLogger::default());
        assert!(!matches!(result, Err(crate::drivers::Error::Storage(_))));
    }
}
