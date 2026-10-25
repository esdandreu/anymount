#![cfg(target_arch = "wasm32")]

//! Implements the browser workspace driver using user-authorized directory handles.

use anymount::domain::{
    ConnectMountError, ConnectStorageError, DisconnectMountError, Driver,
    DriverConfig, Mount, MountConfig, MountError, Storage, StorageConfig,
    UnregisterMountError,
};
use async_trait::async_trait;
use js_sys::{Array, Function, Object, Promise, Reflect};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};
use wasm_bindgen_futures::JsFuture;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
/// Selects the browser workspace driver.
pub struct BrowserDriverConfig;

#[typetag::serde(name = "browser")]
impl DriverConfig for BrowserDriverConfig {
    fn init(&self) -> Box<dyn Driver> {
        Box::new(BrowserDriver::default())
    }
}

#[derive(Default)]
struct BrowserDriver {
    mounts: RefCell<HashMap<String, Rc<dyn BrowserStorage>>>,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl Driver for BrowserDriver {
    async fn mount(
        &self,
        config: &MountConfig,
    ) -> Result<Box<dyn Mount>, MountError> {
        let picker_options = Object::new();
        Reflect::set(
            &picker_options,
            &JsValue::from_str("mode"),
            &JsValue::from_str("readwrite"),
        )
        .map_err(|error| mount_error(config, error))?;
        let args = Array::new();
        args.push(&picker_options);
        let window = js_sys::global();
        let handle = call_async(&window, "showDirectoryPicker", &args)
            .await
            .map_err(|error| mount_error(config, error))?;
        let storage: Rc<dyn BrowserStorage> =
            Rc::new(FileSystemAccessStorage::new(handle));
        self.mounts
            .borrow_mut()
            .insert(config.name.clone(), storage.clone());
        Ok(Box::new(BrowserMount {
            name: config.name.clone(),
            path: config.path.clone(),
            storage,
        }))
    }

    async fn unmount(&self, name: &str) -> Result<(), MountError> {
        let storage = self.mounts.borrow().get(name).cloned();
        if let Some(storage) = storage {
            storage.disconnect().await.map_err(|error| {
                MountError::CannotMountAtPath {
                    path: PathBuf::from(name),
                    message: js_message(&error),
                }
            })?;
            self.mounts.borrow_mut().remove(name);
        }
        Ok(())
    }
}

struct BrowserMount {
    name: String,
    path: PathBuf,
    storage: Rc<dyn BrowserStorage>,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl Mount for BrowserMount {
    fn name(&self) -> &str {
        &self.name
    }

    fn root(&self) -> &Path {
        &self.path
    }

    fn is_connected(&self) -> bool {
        self.storage.is_connected()
    }

    async fn connect(&self) -> Result<(), ConnectMountError> {
        if self.storage.connect(false).await.map_err(|error| {
            ConnectMountError {
                path: self.path.clone(),
                message: js_message(&error),
            }
        })? {
            Ok(())
        } else {
            Err(ConnectMountError {
                path: self.path.clone(),
                message: "browser workspace permission is not granted"
                    .to_owned(),
            })
        }
    }

    async fn disconnect(&self) -> Result<(), DisconnectMountError> {
        self.storage
            .disconnect()
            .await
            .map_err(|error| DisconnectMountError {
                path: self.path.clone(),
                message: js_message(&error),
            })
    }

    async fn unregister(&self) -> Result<(), UnregisterMountError> {
        self.disconnect()
            .await
            .map_err(|error| UnregisterMountError {
                path: self.path.clone(),
                message: error.message,
            })
    }
}

#[async_trait(?Send)]
trait BrowserStorage {
    async fn connect(&self, request_permission: bool) -> Result<bool, JsValue>;
    async fn disconnect(&self) -> Result<(), JsValue>;
    async fn read_text(&self, name: &str) -> Result<String, JsValue>;
    async fn write_text(&self, name: &str, value: &str) -> Result<(), JsValue>;
    fn handle(&self) -> JsValue;
    fn is_connected(&self) -> bool;
}

struct FileSystemAccessStorage {
    handle: JsValue,
    connected: Cell<bool>,
}

impl FileSystemAccessStorage {
    fn new(handle: JsValue) -> Self {
        Self {
            handle,
            connected: Cell::new(false),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl BrowserStorage for FileSystemAccessStorage {
    async fn connect(&self, request_permission: bool) -> Result<bool, JsValue> {
        let options = Object::new();
        Reflect::set(
            &options,
            &JsValue::from_str("mode"),
            &JsValue::from_str("readwrite"),
        )?;
        let mut permission =
            call_async(&self.handle, "queryPermission", &Array::of1(&options))
                .await?;
        if permission.as_string().as_deref() != Some("granted")
            && request_permission
        {
            permission = call_async(
                &self.handle,
                "requestPermission",
                &Array::of1(&options),
            )
            .await?;
        }
        let connected = permission.as_string().as_deref() == Some("granted");
        self.connected.set(connected);
        Ok(connected)
    }

    async fn disconnect(&self) -> Result<(), JsValue> {
        self.connected.set(false);
        Ok(())
    }

    async fn read_text(&self, name: &str) -> Result<String, JsValue> {
        self.ensure_connected()?;
        let file_handle = self.file_handle(name, false).await?;
        let file = call_async(&file_handle, "getFile", &Array::new()).await?;
        let text = call_async(&file, "text", &Array::new()).await?;
        text.as_string()
            .ok_or_else(|| JsValue::from_str("file contents were not text"))
    }

    async fn write_text(&self, name: &str, value: &str) -> Result<(), JsValue> {
        self.ensure_connected()?;
        let file_handle = self.file_handle(name, true).await?;
        let writable =
            call_async(&file_handle, "createWritable", &Array::new()).await?;
        let result = async {
            call_async(
                &writable,
                "write",
                &Array::of1(&JsValue::from_str(value)),
            )
            .await?;
            call_async(&writable, "close", &Array::new()).await
        }
        .await;
        if let Err(error) = result {
            let _ = call_async(&writable, "abort", &Array::new()).await;
            return Err(error);
        }
        Ok(())
    }

    fn handle(&self) -> JsValue {
        self.handle.clone()
    }

    fn is_connected(&self) -> bool {
        self.connected.get()
    }
}

impl FileSystemAccessStorage {
    async fn file_handle(
        &self,
        name: &str,
        create: bool,
    ) -> Result<JsValue, JsValue> {
        let options = Object::new();
        Reflect::set(
            &options,
            &JsValue::from_str("create"),
            &JsValue::from_bool(create),
        )?;
        call_async(
            &self.handle,
            "getFileHandle",
            &Array::of2(&JsValue::from_str(name), &options),
        )
        .await
    }

    fn ensure_connected(&self) -> Result<(), JsValue> {
        if self.is_connected() {
            Ok(())
        } else {
            Err(JsValue::from_str("browser workspace is disconnected"))
        }
    }
}

#[derive(Deserialize, Serialize)]
struct BrowserStorageConfig;

#[typetag::serde(name = "browser-placeholder")]
impl StorageConfig for BrowserStorageConfig {
    fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        Ok(Box::new(BrowserStoragePlaceholder))
    }
}

struct BrowserStoragePlaceholder;

impl Storage for BrowserStoragePlaceholder {}

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
        let mount = Driver::mount(&self.driver, &config)
            .await
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        if let Err(error) = mount.connect().await {
            if let Err(cleanup_error) =
                Driver::unmount(&self.driver, &config.name).await
            {
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
        let storage: Rc<dyn BrowserStorage> =
            Rc::new(FileSystemAccessStorage::new(handle));
        self.driver
            .mounts
            .borrow_mut()
            .insert(name.clone(), storage.clone());
        let mount = BrowserMount {
            name,
            path: PathBuf::from(path),
            storage,
        };
        match mount.connect().await {
            Ok(()) => Ok(true),
            Err(error)
                if error.message
                    == "browser workspace permission is not granted" =>
            {
                Ok(false)
            }
            Err(error) => Err(JsValue::from_str(&error.to_string())),
        }
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
        storage.connect(request_permission).await
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
        storage.disconnect().await
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
        self.storage(&name)?.read_text(&file_name).await
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
        self.storage(&name)?.write_text(&file_name, &value).await
    }

    /// Returns the browser handle for a mounted workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace is not mounted.
    pub fn directory_handle(&self, name: String) -> Result<JsValue, JsValue> {
        Ok(self.storage(&name)?.handle())
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
    fn storage(&self, name: &str) -> Result<Rc<dyn BrowserStorage>, JsValue> {
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

async fn call_async(
    receiver: &JsValue,
    method: &str,
    args: &Array,
) -> Result<JsValue, JsValue> {
    let function: Function =
        Reflect::get(receiver, &JsValue::from_str(method))?
            .dyn_into()
            .map_err(|_| {
                JsValue::from_str("browser API method is unavailable")
            })?;
    let promise: Promise =
        function.apply(receiver, args)?.dyn_into().map_err(|_| {
            JsValue::from_str("browser API did not return a promise")
        })?;
    JsFuture::from(promise).await
}

fn mount_error(config: &MountConfig, error: JsValue) -> MountError {
    MountError::CannotMountAtPath {
        path: config.path.clone(),
        message: js_message(&error),
    }
}

fn js_message(error: &JsValue) -> String {
    Reflect::get(error, &JsValue::from_str("message"))
        .ok()
        .and_then(|message| message.as_string())
        .unwrap_or_else(|| format!("{error:?}"))
}
