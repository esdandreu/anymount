use super::{Error, Result};
use crate::domain::driver::{
    Driver, DriverConfig, ListMountsError, MountError,
};
use crate::domain::{
    ConnectMountError, DisconnectMountError, Mount, Storage,
    UnregisterMountError,
};
use cloud_filter::root::{
    Connection, HydrationType, PopulationType, SecurityId,
    Session as CloudSession, SyncRootId, SyncRootIdBuilder, SyncRootInfo,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf, absolute};

pub const ID_PREFIX: &str = "Anymount";

/// Configuration for the Windows CloudFilter driver.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct WindowsDriverConfig;

#[typetag::serde(name = "windows")]
impl DriverConfig for WindowsDriverConfig {
    fn init(&self) -> Box<dyn Driver> {
        Box::new(WindowsDriver)
    }
}

inventory::submit! {
    crate::domain::driver::DefaultDriverConfig {
        priority: 0,
        config: &WindowsDriverConfig,
    }
}

/// Creates Windows CloudFilter mounts.
pub struct WindowsDriver;

impl Driver for WindowsDriver {
    fn mount(
        &self,
        name: String,
        path: PathBuf,
        storage: Box<dyn Storage>,
    ) -> std::result::Result<Box<dyn Mount>, MountError> {
        WindowsMount::new(name, path.clone(), storage)
            .map(|mount| Box::new(mount) as Box<dyn Mount>)
            .map_err(|source| MountError::CannotMountAtPath {
                path,
                message: source.to_string(),
            })
    }

    fn list_mounts(
        &self,
    ) -> std::result::Result<
        Box<dyn Iterator<Item = Box<dyn Mount>>>,
        ListMountsError,
    > {
        Ok(Box::new(std::iter::empty()))
    }
}

/// A registered Windows sync root with an optional live connection.
pub struct WindowsMount<S: Storage> {
    name: String,
    path: PathBuf,
    id: SyncRootId,
    connection: Mutex<Option<Connection<super::Callbacks<S>>>>,
}

impl<S: Storage + 'static> WindowsMount<S> {
    pub fn new(name: String, path: PathBuf, storage: S) -> Result<Self> {
        let security_id = SecurityId::current_user().map_err(|source| {
            Error::CloudFilterOperation {
                operation: "resolve current user security id",
                source,
            }
        })?;
        if !path.exists() {
            std::fs::create_dir_all(&path).map_err(|source| Error::Io {
                operation: "create mount path",
                path: path.clone(),
                source,
            })?;
        }
        tracing::info!(path = %path.display(), "mounting Windows sync root");
        let path = absolute(&path).map_err(|source| Error::Io {
            operation: "resolve mount path",
            path: path.clone(),
            source,
        })?;
        let driver_name = format!("{ID_PREFIX}|{name}");
        let id = SyncRootIdBuilder::new(driver_name)
            .user_security_id(security_id)
            .build();

        let is_registered = id.is_registered().map_err(|source| {
            Error::CloudFilterOperation {
                operation: "check sync root registration",
                source,
            }
        })?;
        if !is_registered {
            let sync_root_info = SyncRootInfo::default()
                .with_display_name(&name)
                .with_icon("%SystemRoot%\\system32\\charmap.exe,0")
                .with_version(env!("CARGO_PKG_VERSION"))
                .with_hydration_type(HydrationType::Full)
                .with_population_type(PopulationType::Full)
                .with_path(&path)
                .map_err(|source| Error::CloudFilterOperation {
                    operation: "build sync root info",
                    source,
                })?;
            id.register(sync_root_info).map_err(|source| {
                Error::CloudFilterOperation {
                    operation: "register sync root",
                    source,
                }
            })?;
        }

        let connection = CloudSession::new()
            .connect(&path, super::Callbacks::new(path.clone(), storage))
            .map_err(|source| Error::CloudFilterOperation {
                operation: "connect to sync root",
                source,
            })?;

        Ok(Self {
            name,
            path,
            id,
            connection: Mutex::new(Some(connection)),
        })
    }
}

impl<S: Storage + 'static> Mount for WindowsMount<S> {
    fn name(&self) -> &str {
        &self.name
    }

    fn root(&self) -> &Path {
        &self.path
    }

    fn is_connected(&self) -> bool {
        self.connection.lock().is_some()
    }

    fn connect(&self) -> std::result::Result<(), ConnectMountError> {
        if self.is_connected() {
            Ok(())
        } else {
            Err(ConnectMountError {
                path: self.path.clone(),
                message:
                    "a disconnected CloudFilter mount cannot be reconnected"
                        .to_owned(),
            })
        }
    }

    fn disconnect(&self) -> std::result::Result<(), DisconnectMountError> {
        self.connection.lock().take();
        Ok(())
    }

    fn unregister(&self) -> std::result::Result<(), UnregisterMountError> {
        self.connection.lock().take();
        self.id.unregister().map_err(|source| UnregisterMountError {
            path: self.path.clone(),
            message: source.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{WindowsDriver, WindowsDriverConfig};
    use crate::domain::{Driver, DriverConfig};

    #[test]
    fn config_initializes_empty_driver() {
        let driver = WindowsDriverConfig.init();

        assert_eq!(driver.list_mounts().expect("list mounts").count(), 0);
    }

    #[test]
    fn driver_lists_no_mounts_without_native_discovery() {
        let driver = WindowsDriver;

        assert_eq!(driver.list_mounts().expect("list mounts").count(), 0);
    }
}
