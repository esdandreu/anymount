use crate::domain::driver::{Driver, DriverConfig, MountError};
use crate::domain::{Mount, Storage};
use crate::drivers::fuse::fuse_mount::FuseMount;
use crate::drivers::fuse::{NoCacheFsCache, StorageFilesystem};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
}
