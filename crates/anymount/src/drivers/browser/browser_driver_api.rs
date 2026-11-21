use super::{
    BrowserDriver, BrowserMount, BrowserStorageConfig, BrowserWorkspace,
    ConnectPermissionError, FileSystemAccessStorage,
};
use crate::domain::{
    Driver, Mount, MountError, StartWriteFileError, Storage, StoragePath,
    WriteFileError,
};
use crate::drivers::browser::OneDriveBrowserApi;
use futures::io::AsyncWriteExt;
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

    /// Mounts a workspace sharing the host's existing OneDrive authorization.
    ///
    /// # Errors
    ///
    /// Returns a sign-in error when the shared session has no credentials.
    pub fn mount_onedrive(
        &self,
        name: String,
        path: String,
        authorization: &OneDriveBrowserApi,
    ) -> Result<(), JsValue> {
        let storage = authorization.session().connect().map_err(|_| {
            let error = js_sys::Error::new("sign-in is required");
            error.set_name("SignInRequiredError");
            JsValue::from(error)
        })?;
        self.driver
            .register(name, PathBuf::from(path), Rc::new(storage), None);
        Ok(())
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
        let path = PathBuf::from(path);
        let provider =
            BrowserStorageConfig
                .pick_directory()
                .await
                .map_err(|error| {
                    JsValue::from_str(
                        &MountError::CannotMountAtPath {
                            path: path.clone(),
                            message: error.to_string(),
                        }
                        .to_string(),
                    )
                })?;
        let handle = provider.handle.clone();
        let previous = self.driver.mounts.borrow().get(&name).cloned();
        let mount = self.driver.register(
            name.clone(),
            path,
            Rc::new(provider),
            Some(handle),
        );
        if let Err(error) = mount.connect().await {
            let cleanup = Driver::unmount(&self.driver, &name).await;
            if let Some(previous) = previous {
                self.driver.mounts.borrow_mut().insert(name, previous);
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
            Rc::new(FileSystemAccessStorage::new(handle.clone()));
        let workspace = BrowserWorkspace::new(storage, Some(handle));
        let mount = BrowserMount {
            name: name.clone(),
            path: PathBuf::from(path),
            workspace: workspace.clone(),
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
        self.driver.mounts.borrow_mut().insert(name, workspace);
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
        let workspace = self.workspace(&name)?;
        workspace
            .connect(request_permission)
            .await
            .map_err(connect_js_error)
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
        self.workspace(&name)?.disconnect();
        Ok(())
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
        let storage = self.workspace(&name)?;
        let mut writer = Vec::new();
        let mut offset = 0;
        loop {
            let end = offset + 65536;
            let count = storage
                .read_file_at(path.clone(), &mut writer, offset..end)
                .await
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            offset += count;
            if count < 65536 {
                break;
            }
        }
        String::from_utf8(writer)
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
        let workspace = self.workspace(&name)?;
        let mut session = workspace
            .start_write_file(path)
            .await
            .map_err(start_write_file_js_error)?;
        let result = async {
            session
                .write_all(value.as_bytes())
                .await
                .map_err(WriteFileError::from)?;
            session.finish().await
        }
        .await;
        if result.is_err() {
            let _ = session.abort().await;
        }
        result.map_err(write_js_error)
    }

    /// Returns the browser handle for a mounted workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace is not mounted.
    pub fn directory_handle(&self, name: String) -> Result<JsValue, JsValue> {
        let mounts = self.driver.mounts.borrow();
        let workspace = mounts.get(&name).ok_or_else(|| {
            JsValue::from_str("browser workspace is not mounted")
        })?;
        workspace.handle.clone().ok_or_else(|| {
            JsValue::from_str("workspace is not a browser directory")
        })
    }

    /// Reports whether a browser workspace is connected.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace is not mounted.
    pub fn is_connected(&self, name: String) -> Result<bool, JsValue> {
        Ok(self.workspace(&name)?.is_connected())
    }
}

impl BrowserDriverApi {
    fn workspace(&self, name: &str) -> Result<BrowserWorkspace, JsValue> {
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

fn connect_js_error(error: ConnectPermissionError) -> JsValue {
    let name = match error {
        ConnectPermissionError::PermissionDenied => Some("NotAllowedError"),
        _ => None,
    };
    js_error(&error.to_string(), name)
}

fn start_write_file_js_error(error: StartWriteFileError) -> JsValue {
    let name = match error {
        StartWriteFileError::NotFound => Some("NotFoundError"),
        StartWriteFileError::PermissionDenied => Some("NotAllowedError"),
        _ => None,
    };
    js_error(&error.to_string(), name)
}

fn write_js_error(error: WriteFileError) -> JsValue {
    let name = match error {
        WriteFileError::NotFound => Some("NotFoundError"),
        WriteFileError::PermissionDenied => Some("NotAllowedError"),
        _ => None,
    };
    js_error(&error.to_string(), name)
}

fn js_error(message: &str, name: Option<&str>) -> JsValue {
    let result = js_sys::Error::new(message);
    if let Some(name) = name {
        result.set_name(name);
    }
    result.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn operation_errors_preserve_names_and_messages() {
        let cases = [
            (
                connect_js_error(ConnectPermissionError::PermissionDenied),
                "NotAllowedError",
                "storage permission denied",
            ),
            (
                start_write_file_js_error(StartWriteFileError::NotFound),
                "NotFoundError",
                "storage entry not found",
            ),
            (
                start_write_file_js_error(
                    StartWriteFileError::PermissionDenied,
                ),
                "NotAllowedError",
                "storage permission denied",
            ),
            (
                start_write_file_js_error(StartWriteFileError::NotAFile),
                "Error",
                "path is not a file",
            ),
            (
                write_js_error(WriteFileError::NotFound),
                "NotFoundError",
                "storage entry not found",
            ),
            (
                write_js_error(WriteFileError::PermissionDenied),
                "NotAllowedError",
                "storage permission denied",
            ),
            (
                write_js_error(WriteFileError::NotAFile),
                "Error",
                "path is not a file",
            ),
        ];
        for (value, name, message) in cases {
            let error: js_sys::Error = value.unchecked_into();
            assert_eq!(error.name(), name);
            assert_eq!(error.message(), message);
        }
    }
}
