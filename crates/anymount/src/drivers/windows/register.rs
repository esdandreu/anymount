use super::WindowsDriver;
use super::{Error, Result};
use crate::storages::Storage;
use crate::Logger;
use std::path::{Path, PathBuf};
use windows::core::HSTRING;
use windows::Storage::Provider::*;
use windows::Storage::StorageFolder;
use windows::Win32::Storage::CloudFilters::{
    CfConnectSyncRoot, CfDisconnectSyncRoot, CF_CONNECTION_KEY, CF_CONNECT_FLAGS,
};

#[derive(Debug, Clone)]
pub struct RegistrationConfig {
    pub sync_root_path: PathBuf,
    pub display_name: String,
    pub provider_id: String,
    pub provider_version: String,
    pub icon_resource: Option<String>,
    pub show_overlays: bool,
    pub auto_populate: bool,
    pub hydration_policy: HydrationPolicy,
}

#[derive(Debug, Clone, Copy)]
pub enum HydrationPolicy {
    Progressive,
    Full,
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

impl<S: Storage, L: Logger> WindowsDriver<S, L> {
    pub fn register_sync_root(&self, config: &RegistrationConfig) -> Result<()> {
        self.logger.info(format!(
            "Registering sync root: {} at {:?}",
            config.display_name, config.sync_root_path
        ));

        std::fs::create_dir_all(&config.sync_root_path).map_err(|source| Error::Io {
            operation: "create sync root directory",
            path: config.sync_root_path.clone(),
            source,
        })?;

        let sync_root_info =
            StorageProviderSyncRootInfo::new().map_err(|source| Error::WindowsOperation {
                operation: "create sync root info",
                source,
            })?;

        let folder_name = config
            .sync_root_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Anymount");
        sync_root_info
            .SetId(&HSTRING::from(folder_name))
            .map_err(|source| Error::WindowsOperation {
                operation: "set sync root id",
                source,
            })?;

        let path_hstring = HSTRING::from(config.sync_root_path.to_string_lossy().as_ref());
        let folder = StorageFolder::GetFolderFromPathAsync(&path_hstring)
            .map_err(|source| Error::WindowsOperation {
                operation: "get sync root folder async",
                source,
            })?
            .get()
            .map_err(|source| Error::WindowsOperation {
                operation: "get sync root folder",
                source,
            })?;
        sync_root_info
            .SetPath(&folder)
            .map_err(|source| Error::WindowsOperation {
                operation: "set sync root path",
                source,
            })?;

        sync_root_info
            .SetDisplayNameResource(&HSTRING::from(&config.display_name))
            .map_err(|source| Error::WindowsOperation {
                operation: "set display name",
                source,
            })?;

        if let Some(icon) = &config.icon_resource {
            sync_root_info
                .SetIconResource(&HSTRING::from(icon))
                .map_err(|source| Error::WindowsOperation {
                    operation: "set icon resource",
                    source,
                })?;
        } else {
            sync_root_info
                .SetIconResource(&HSTRING::from("%SystemRoot%\\system32\\shell32.dll,13"))
                .map_err(|source| Error::WindowsOperation {
                    operation: "set default icon resource",
                    source,
                })?;
        }

        let hydration_policy = match config.hydration_policy {
            HydrationPolicy::Progressive => StorageProviderHydrationPolicy::Progressive,
            HydrationPolicy::Full => StorageProviderHydrationPolicy::Full,
            HydrationPolicy::AlwaysFull => StorageProviderHydrationPolicy::AlwaysFull,
        };
        sync_root_info
            .SetHydrationPolicy(hydration_policy)
            .map_err(|source| Error::WindowsOperation {
                operation: "set hydration policy",
                source,
            })?;
        sync_root_info
            .SetHydrationPolicyModifier(StorageProviderHydrationPolicyModifier::None)
            .map_err(|source| Error::WindowsOperation {
                operation: "set hydration policy modifier",
                source,
            })?;

        let population_policy = if config.auto_populate {
            StorageProviderPopulationPolicy::Full
        } else {
            StorageProviderPopulationPolicy::AlwaysFull
        };
        sync_root_info
            .SetPopulationPolicy(population_policy)
            .map_err(|source| Error::WindowsOperation {
                operation: "set population policy",
                source,
            })?;

        sync_root_info
            .SetInSyncPolicy(StorageProviderInSyncPolicy::PreserveInsyncForSyncEngine)
            .map_err(|source| Error::WindowsOperation {
                operation: "set in-sync policy",
                source,
            })?;
        sync_root_info
            .SetHardlinkPolicy(StorageProviderHardlinkPolicy::None)
            .map_err(|source| Error::WindowsOperation {
                operation: "set hardlink policy",
                source,
            })?;
        sync_root_info
            .SetShowSiblingsAsGroup(false)
            .map_err(|source| Error::WindowsOperation {
                operation: "set show siblings as group",
                source,
            })?;
        sync_root_info
            .SetVersion(&HSTRING::from(config.provider_version.as_str()))
            .map_err(|source| Error::WindowsOperation {
                operation: "set provider version",
                source,
            })?;

        match StorageProviderSyncRootManager::Register(&sync_root_info) {
            Ok(_) => {
                self.logger.info("Sync root registered successfully");
                Ok(())
            }
            Err(e) => {
                self.logger
                    .error(format!("Failed to register sync root: {:?}", e));
                Err(Error::WindowsOperation {
                    operation: "register sync root",
                    source: e,
                })
            }
        }
    }

    pub fn unregister_sync_root(&self, sync_root_path: &Path) -> Result<()> {
        self.logger
            .info(format!("Unregistering sync root: {:?}", sync_root_path));

        let folder_name = sync_root_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Anymount");

        match StorageProviderSyncRootManager::Unregister(&HSTRING::from(folder_name)) {
            Ok(_) => {
                self.logger.info("Sync root unregistered successfully");
                Ok(())
            }
            Err(e) => {
                let err_str = format!("{:?}", e);

                if err_str.contains("0x8007017C") {
                    self.logger
                        .error(format!("Failed to unregister sync root: {:?}", e));
                    self.logger
                        .error("The sync root appears to have an active connection.");
                    self.logger.error(
                        "Please ensure no applications are using the sync root and try again.",
                    );
                    self.logger.error(
                        "You may need to restart your computer to fully release the connection.",
                    );
                    Err(Error::WindowsOperation {
                        operation: "unregister sync root (active connection detected)",
                        source: e,
                    })
                } else if err_str.contains("0x8007018B") {
                    self.logger
                        .error(format!("Failed to unregister sync root: {:?}", e));
                    self.logger
                        .error("Access denied. Make sure you're running as Administrator.");
                    Err(Error::WindowsOperation {
                        operation: "unregister sync root (access denied)",
                        source: e,
                    })
                } else {
                    self.logger
                        .error(format!("Failed to unregister sync root: {:?}", e));
                    Err(Error::WindowsOperation {
                        operation: "unregister sync root",
                        source: e,
                    })
                }
            }
        }
    }

    pub fn is_sync_root_registered(&self, _path: &Path) -> Result<bool> {
        let id = HSTRING::from("Anymount");

        match StorageProviderSyncRootManager::GetSyncRootInformationForId(&id) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    pub fn connect_sync_root(&self, sync_root_path: &Path) -> Result<CF_CONNECTION_KEY> {
        self.logger
            .info(format!("Connecting to sync root: {:?}", sync_root_path));

        unsafe {
            let path = HSTRING::from(sync_root_path.to_string_lossy().as_ref());

            match CfConnectSyncRoot(&path, std::ptr::null(), None, CF_CONNECT_FLAGS(0)) {
                Ok(key) => {
                    self.logger.info("Connected to sync root successfully");
                    Ok(key)
                }
                Err(e) => {
                    self.logger
                        .error(format!("Failed to connect to sync root: {:?}", e));
                    Err(Error::WindowsOperation {
                        operation: "connect sync root",
                        source: e,
                    })
                }
            }
        }
    }

    pub fn disconnect_sync_root(&self, connection_key: CF_CONNECTION_KEY) -> Result<()> {
        self.logger.info("Disconnecting from sync root");

        unsafe {
            match CfDisconnectSyncRoot(connection_key) {
                Ok(_) => {
                    self.logger.info("Disconnected successfully");
                    Ok(())
                }
                Err(e) => {
                    self.logger.warn(format!("Failed to disconnect: {:?}", e));
                    Ok(())
                }
            }
        }
    }
}
