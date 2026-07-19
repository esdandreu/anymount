// Copyright 2026 Dotphoton AG

use crate::domain::driver::{
    Driver, DriverConfig, ListMountsError, MountError,
};
use crate::domain::{
    ConnectMountError, DisconnectMountError, Mount, Storage,
    UnregisterMountError,
};
use crate::drivers::fuse::{NoCacheFsCache, StorageFilesystem};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Configuration for the macOS FUSE driver.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct FuseDriverConfig;

#[typetag::serde(name = "fuse")]
impl DriverConfig for FuseDriverConfig {
    fn init(&self) -> Box<dyn Driver> {
        Box::new(FuseDriver::default())
    }
}

inventory::submit! {
    crate::domain::driver::DefaultDriverConfig {
        priority: 0,
        config: &FuseDriverConfig,
    }
}

/// Creates macOS FUSE mounts.
#[derive(Default)]
pub struct FuseDriver;

impl Driver for FuseDriver {
    fn mount(
        &self,
        name: String,
        path: PathBuf,
        storage: Box<dyn Storage>,
    ) -> Result<Box<dyn Mount>, MountError> {
        std::fs::create_dir_all(&path).map_err(|source| {
            MountError::CannotMountAtPath {
                path: path.clone(),
                message: source.to_string(),
            }
        })?;
        let path = path.canonicalize().map_err(|source| {
            MountError::CannotMountAtPath {
                path: path.clone(),
                message: source.to_string(),
            }
        })?;
        let filesystem = StorageFilesystem::new_with_cache(
            storage,
            Arc::new(NoCacheFsCache::new()),
        );
        let session =
            fuser::spawn_mount2(filesystem, &path, &fuser::Config::default())
                .map_err(|source| MountError::CannotMountAtPath {
                path: path.clone(),
                message: source.to_string(),
            })?;
        Ok(Box::new(FuseMount::new(name, path, session)))
    }

    fn list_mounts(
        &self,
    ) -> Result<Box<dyn Iterator<Item = Box<dyn Mount>>>, ListMountsError> {
        Ok(Box::new(std::iter::empty()))
    }
}

/// A live macOS FUSE mount.
pub struct FuseMount {
    name: String,
    path: PathBuf,
    session: Mutex<Option<fuser::BackgroundSession>>,
}

impl FuseMount {
    pub(crate) fn new(
        name: String,
        path: PathBuf,
        session: fuser::BackgroundSession,
    ) -> Self {
        Self {
            name,
            path,
            session: Mutex::new(Some(session)),
        }
    }
}

impl Mount for FuseMount {
    fn name(&self) -> &str {
        &self.name
    }

    fn root(&self) -> &Path {
        &self.path
    }

    fn is_connected(&self) -> bool {
        self.session.lock().is_some()
    }

    fn connect(&self) -> Result<(), ConnectMountError> {
        if self.is_connected() {
            Ok(())
        } else {
            Err(ConnectMountError {
                path: self.path.clone(),
                message: "an unmounted FUSE session cannot be reconnected"
                    .to_owned(),
            })
        }
    }

    fn disconnect(&self) -> Result<(), DisconnectMountError> {
        self.session.lock().take();
        Ok(())
    }

    fn unregister(&self) -> Result<(), UnregisterMountError> {
        self.session.lock().take();
        std::fs::remove_dir(&self.path).map_err(|source| {
            UnregisterMountError {
                path: self.path.clone(),
                message: source.to_string(),
            }
        })?;
        Ok(())
    }
}

impl std::fmt::Debug for FuseMount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FuseMount")
            .field("name", &self.name)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::FuseDriver;
    use crate::domain::Driver;

    #[test]
    fn list_mounts_is_empty_without_native_discovery() {
        let driver = FuseDriver;

        let mounts = driver.list_mounts().expect("list mounts");

        assert_eq!(mounts.count(), 0);
    }
}
