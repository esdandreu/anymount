use super::windows_driver::ID_PREFIX;
use super::{Error, Result};
use crate::domain::driver::Driver;
use crate::Logger;
use windows::{
    Foundation::Collections::IVectorView,
    Storage::Provider::{StorageProviderSyncRootInfo, StorageProviderSyncRootManager},
};

pub fn cleanup_registry<L: Logger>(specs: &[Driver], logger: &L) -> Result<()> {
    _cleanup_registry::<StorageProviderSyncRootManager, L>(specs, logger)
}

trait RegistryManager {
    fn get_currently_registered() -> Result<IVectorView<StorageProviderSyncRootInfo>>;
    fn unregister(id: &windows::core::HSTRING) -> Result<()>;
}

impl RegistryManager for StorageProviderSyncRootManager {
    fn get_currently_registered() -> Result<IVectorView<StorageProviderSyncRootInfo>> {
        StorageProviderSyncRootManager::GetCurrentSyncRoots().map_err(|source| {
            Error::WindowsOperation {
                operation: "get current sync roots",
                source,
            }
        })
    }
    fn unregister(id: &windows::core::HSTRING) -> Result<()> {
        StorageProviderSyncRootManager::Unregister(id).map_err(|source| Error::WindowsOperation {
            operation: "unregister sync root",
            source,
        })
    }
}

fn _cleanup_registry<Registry: RegistryManager, L: Logger>(
    specs: &[Driver],
    logger: &L,
) -> Result<()> {
    let sync_roots = Registry::get_currently_registered()?;
    for sync_root in sync_roots {
        let id = match sync_root.Id() {
            Ok(id) => id,
            Err(_) => continue,
        };

        if !id.to_string().starts_with(ID_PREFIX) {
            continue;
        }

        let sync_root_path = match get_sync_root_path(&sync_root) {
            Ok(path) => path,
            Err(_) => {
                logger.info(format!(
                    "Failed to get path for sync root {}, skipping",
                    id.to_string()
                ));
                continue;
            }
        };

        if is_path_configured(&sync_root_path, specs) {
            continue;
        }

        match Registry::unregister(&id) {
            Ok(()) => logger.info(format!(
                "Unregistered non-configured sync root {} at {}",
                id.to_string(),
                sync_root_path
            )),
            Err(e) => logger.error(format!(
                "Failed to unregister {} at {} {:?}",
                id.to_string(),
                sync_root_path,
                e
            )),
        }
    }

    Ok(())
}

fn is_path_configured(path: &str, specs: &[Driver]) -> bool {
    specs.iter().any(|spec| {
        let driver_path = spec.path.to_string_lossy().to_string();
        path.eq_ignore_ascii_case(&driver_path)
    })
}

fn get_sync_root_path(sync_root: &StorageProviderSyncRootInfo) -> Result<String> {
    let folder = sync_root.Path().map_err(|source| Error::WindowsOperation {
        operation: "get sync root folder",
        source,
    })?;
    let path = folder.Path().map_err(|source| Error::WindowsOperation {
        operation: "get sync root path",
        source,
    })?;
    Ok(path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoOpLogger;

    struct FailingRegistryManager;

    impl RegistryManager for FailingRegistryManager {
        fn get_currently_registered() -> Result<IVectorView<StorageProviderSyncRootInfo>> {
            Err(Error::WindowsOperation {
                operation: "get current sync roots",
                source: windows::core::Error::from_hresult(windows::core::HRESULT(
                    0x80004005u32 as i32,
                )),
            })
        }

        fn unregister(_id: &windows::core::HSTRING) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn cleanup_registry_returns_get_current_sync_roots_error() {
        let err = _cleanup_registry::<FailingRegistryManager, _>(&[], &NoOpLogger)
            .expect_err("cleanup should fail");
        assert!(matches!(err, super::Error::WindowsOperation { .. }));
    }
}
