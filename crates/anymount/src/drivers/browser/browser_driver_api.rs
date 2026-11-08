use super::{
    BrowserDriver, BrowserMount, BrowserStorageConfig, FileSystemAccessStorage,
};
use crate::domain::{
    Driver, Mount, MountConfig, Storage, StorageError, StoragePath,
};
use std::path::PathBuf;
use std::rc::Rc;
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};

#[wasm_bindgen]
/// Exposes the browser driver to a static JavaScript application.
pub struct BrowserDriverApi {
    driver: BrowserDriver,
}

#[wasm_bindgen]
impl BrowserDriverApi {
    #[wasm_bindgen(constructor)]
    /// Creates a browser driver instance.
    pub fn new() -> Self {
        Self {
            driver: BrowserDriver::default(),
        }
    }

    /// Selects a directory and registers it as a browser workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the picker, permission check, or cleanup fails.
    pub async fn mount_workspace(
        &self,
        name: String,
        path: String,
    ) -> Result<(), JsValue> {
        let config = MountConfig {
            name,
            path: PathBuf::from(path),
            storage: Box::new(BrowserStorageConfig),
        };
        let previous = self.driver.mounts.borrow().get(&config.name).cloned();
        let mount = Driver::mount(&self.driver, &config)
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        if let Err(error) = mount.connect().await {
            let cleanup = Driver::unmount(&self.driver, &config.name).await;
            if let Some(previous) = previous {
                self.driver
                    .mounts
                    .borrow_mut()
                    .insert(config.name.clone(), previous);
            }
            if let Err(cleanup_error) = cleanup {
                return Err(JsValue::from_str(&format!(
                    "{error}; cleanup failed: {cleanup_error}"
                )));
            }
            return Err(JsValue::from_str(&error.to_string()));
        }
        Ok(())
    }

    /// Restores a workspace from a saved directory handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the browser cannot query the handle's permission.
    pub async fn restore_workspace(
        &self,
        name: String,
        path: String,
        handle: JsValue,
    ) -> Result<bool, JsValue> {
        let storage: Rc<dyn Storage> =
            Rc::new(FileSystemAccessStorage::new(handle));
        let mount = BrowserMount {
            name: name.clone(),
            path: PathBuf::from(path),
            storage: storage.clone(),
        };
        let connected = match mount.connect().await {
            Ok(()) => true,
            Err(error)
                if error.message
                    == "browser workspace permission is not granted" =>
            {
                false
            }
            Err(error) => return Err(JsValue::from_str(&error.to_string())),
        };
        self.driver.mounts.borrow_mut().insert(name, storage);
        Ok(connected)
    }

    /// Connects a saved workspace and optionally requests permission.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace is not mounted or access fails.
    pub async fn connect_workspace(
        &self,
        name: String,
        request_permission: bool,
    ) -> Result<bool, JsValue> {
        let storage = self.storage(&name)?;
        storage
            .connect(request_permission)
            .await
            .map_err(to_js_error)
    }

    /// Disconnects a workspace while keeping its selected handle available.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace is not mounted or disconnect fails.
    pub async fn disconnect_workspace(
        &self,
        name: String,
    ) -> Result<(), JsValue> {
        let storage = self.storage(&name)?;
        storage.disconnect().await.map_err(to_js_error)
    }

    /// Unregisters a browser workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if disconnection fails.
    pub async fn unmount_workspace(&self, name: String) -> Result<(), JsValue> {
        Driver::unmount(&self.driver, &name)
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    /// Reads UTF-8 text from a file in the connected workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace is disconnected or the file cannot be read.
    pub async fn read_text(
        &self,
        name: String,
        file_name: String,
    ) -> Result<String, JsValue> {
        let path = StoragePath::try_from(PathBuf::from(file_name))
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let storage = self.storage(&name)?;
        let mut bytes = Vec::new();
        loop {
            let chunk = storage
                .read_range(path.clone(), bytes.len() as u64, 65536)
                .await
                .map_err(to_js_error)?;
            let length = chunk.len();
            bytes.extend(chunk);
            if length == 0 {
                break;
            }
        }
        String::from_utf8(bytes)
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    /// Writes UTF-8 text to a file in the connected workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace is disconnected or the file cannot be written.
    pub async fn write_text(
        &self,
        name: String,
        file_name: String,
        value: String,
    ) -> Result<(), JsValue> {
        let path = StoragePath::try_from(PathBuf::from(file_name))
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.storage(&name)?
            .write(path, value.as_bytes())
            .await
            .map_err(to_js_error)
    }

    /// Returns the browser handle for a mounted workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace is not mounted.
    pub fn directory_handle(&self, name: String) -> Result<JsValue, JsValue> {
        let storage = self.storage(&name)?;
        let provider = storage
            .as_any()
            .downcast_ref::<FileSystemAccessStorage>()
            .ok_or_else(|| {
                JsValue::from_str("workspace is not a browser directory")
            })?;
        Ok(provider.handle.clone())
    }

    /// Reports whether a browser workspace is connected.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace is not mounted.
    pub fn is_connected(&self, name: String) -> Result<bool, JsValue> {
        Ok(self.storage(&name)?.is_connected())
    }
}

impl BrowserDriverApi {
    fn storage(&self, name: &str) -> Result<Rc<dyn Storage>, JsValue> {
        self.driver
            .mounts
            .borrow()
            .get(name)
            .cloned()
            .ok_or_else(|| {
                JsValue::from_str("browser workspace is not mounted")
            })
    }
}

fn to_js_error(error: StorageError) -> JsValue {
    let result = js_sys::Error::new(&error.to_string());
    match error {
        StorageError::NotFound => result.set_name("NotFoundError"),
        StorageError::PermissionDenied => result.set_name("NotAllowedError"),
        _ => {}
    }
    result.into()
}
