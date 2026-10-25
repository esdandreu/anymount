use crate::domain::driver::{Driver, DriverConfig, MountError};
use crate::domain::{Mount, MountConfig};
use crate::drivers::fuse::fuse_mount::FuseMount;
use crate::drivers::fuse::{NoCacheFsCache, StorageFilesystem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct FuseDriverConfig;

#[typetag::serde(name = "fuse")]
impl DriverConfig for FuseDriverConfig {
    fn init(&self) -> Box<dyn Driver> {
        Box::new(FuseDriver)
    }
}

inventory::submit! {
    crate::domain::driver::DefaultDriverConfig {
        priority: 0,
        config: &FuseDriverConfig,
    }
}

#[derive(Default)]
pub struct FuseDriver;

#[async_trait]
impl Driver for FuseDriver {
    async fn mount(
        &self,
        config: &MountConfig,
    ) -> Result<Box<dyn Mount>, MountError> {
        let name = config.name.clone();
        let path = config.path.clone();
        let storage = config.storage.connect()?;
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
        let mut config = fuser::Config::default();
        config
            .mount_options
            .push(fuser::MountOption::FSName(format!("anymount:{name}")));
        let session = fuser::spawn_mount2(filesystem, &path, &config).map_err(
            |source| MountError::CannotMountAtPath {
                path: path.clone(),
                message: source.to_string(),
            },
        )?;
        Ok(Box::new(FuseMount::new(name, path, session)))
    }

    async fn unmount(&self, _name: &str) -> Result<(), MountError> {
        Ok(())
    }
}
