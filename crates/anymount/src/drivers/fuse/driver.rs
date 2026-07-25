// Copyright 2026 Dotphoton AG

use crate::domain::driver::{
    Driver, DriverConfig, ListMountsError, MountError, MountStatus,
    MountStatusError,
};
use crate::domain::{
    ConnectMountError, DisconnectMountError, Mount, Storage,
    UnregisterMountError,
};
use crate::drivers::fuse::{NoCacheFsCache, StorageFilesystem};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::ffi::CStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Configuration for the macOS FUSE driver.
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
        let mut config = fuser::Config::default();
        config
            .mount_options
            .push(fuser::MountOption::FSName(mount_source(&name)));
        let session = fuser::spawn_mount2(filesystem, &path, &config).map_err(
            |source| MountError::CannotMountAtPath {
                path: path.clone(),
                message: source.to_string(),
            },
        )?;
        Ok(Box::new(FuseMount::new(name, path, session)))
    }

    fn is_mounted(
        &self,
        name: &str,
        expected_root: &Path,
    ) -> Result<MountStatus, MountStatusError> {
        let mounts = native_mounts().map_err(|source| MountStatusError {
            name: name.to_owned(),
            message: source.to_string(),
        })?;
        let expected_root = expected_root
            .canonicalize()
            .unwrap_or_else(|_| expected_root.to_path_buf());

        Ok(status_for_mounts(
            mounts
                .iter()
                .map(|mount| (mount.source.as_str(), mount.root.as_path())),
            name,
            &expected_root,
        ))
    }

    fn list_mounts(
        &self,
    ) -> Result<Box<dyn Iterator<Item = Box<dyn Mount>>>, ListMountsError> {
        Ok(Box::new(std::iter::empty()))
    }
}

const MOUNT_SOURCE_PREFIX: &str = "anymount:";

fn mount_source(name: &str) -> String {
    format!("{MOUNT_SOURCE_PREFIX}{name}")
}

fn status_for_mounts<'a>(
    mut mounts: impl Iterator<Item = (&'a str, &'a Path)>,
    name: &str,
    expected_root: &Path,
) -> MountStatus {
    let expected_source = mount_source(name);
    mounts
        .find(|(source, _)| *source == expected_source)
        .map(|(_, root)| {
            if root == expected_root {
                MountStatus::MountedAtExpectedRoot
            } else {
                MountStatus::MountedAtDifferentRoot
            }
        })
        .unwrap_or(MountStatus::NotMounted)
}

struct NativeMount {
    source: String,
    root: PathBuf,
}

fn native_mounts() -> std::io::Result<Vec<NativeMount>> {
    let mut mounts = std::ptr::null_mut();

    // SAFETY: `getmntinfo` initializes `mounts` to an array containing the
    // returned number of `statfs` entries. The entries are copied before any
    // subsequent call can invalidate libc's static buffer.
    let count = unsafe { libc::getmntinfo(&mut mounts, libc::MNT_NOWAIT) };
    if count == 0 {
        return Err(std::io::Error::last_os_error());
    }

    // SAFETY: A successful `getmntinfo` call guarantees that `mounts` points
    // to `count` initialized entries for the duration described above.
    let mounts = unsafe { std::slice::from_raw_parts(mounts, count as usize) };
    mounts
        .iter()
        .map(|mount| {
            // SAFETY: Darwin's `statfs` fields are fixed-size, null-terminated
            // C strings populated by the kernel.
            let source = unsafe {
                CStr::from_ptr(mount.f_mntfromname.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            };
            // SAFETY: The same `statfs` string guarantee applies here.
            let root = unsafe { CStr::from_ptr(mount.f_mntonname.as_ptr()) };
            let root = std::ffi::OsStr::from_bytes(root.to_bytes())
                .to_owned()
                .into();
            Ok(NativeMount { source, root })
        })
        .collect()
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
    use super::{FuseDriver, mount_source, status_for_mounts};
    use crate::domain::{Driver, MountStatus};
    use std::path::Path;

    #[test]
    fn reports_mount_at_expected_root() {
        let source = mount_source("documents");
        let mounts = [(source.as_str(), Path::new("/mnt/documents"))];

        let status = status_for_mounts(
            mounts.into_iter(),
            "documents",
            Path::new("/mnt/documents"),
        );

        assert_eq!(status, MountStatus::MountedAtExpectedRoot);
    }

    #[test]
    fn reports_mount_at_different_root() {
        let source = mount_source("documents");
        let mounts = [(source.as_str(), Path::new("/mnt/old"))];

        let status = status_for_mounts(
            mounts.into_iter(),
            "documents",
            Path::new("/mnt/documents"),
        );

        assert_eq!(status, MountStatus::MountedAtDifferentRoot);
    }

    #[test]
    fn ignores_mounts_owned_by_other_providers() {
        let mounts = [("other", Path::new("/mnt/documents"))];

        let status = status_for_mounts(
            mounts.into_iter(),
            "documents",
            Path::new("/mnt/documents"),
        );

        assert_eq!(status, MountStatus::NotMounted);
    }

    #[test]
    fn list_mounts_is_empty_without_native_reconstruction() {
        let driver = FuseDriver;

        let mounts = driver.list_mounts().expect("list mounts");

        assert_eq!(mounts.count(), 0);
    }
}
