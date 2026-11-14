//! Implements the browser workspace driver using user-authorized directory handles.

#[path = "browser_driver_api.rs"]
mod browser_driver_api;

pub use browser_driver_api::BrowserDriverApi;

use crate::domain::{
    AsyncStorage, ConnectMountError, ConnectStorageError, DisconnectMountError,
    Driver, DriverConfig, Mount, MountConfig, MountError, Storage,
    StorageConfig, StorageError, StoragePath, UnregisterMountError,
};
use async_trait::async_trait;
use js_sys::{Array, Function, Object, Promise, Reflect};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use wasm_bindgen::{JsCast, JsValue};
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

inventory::submit! {
    crate::domain::DefaultDriverConfig {
        priority: 0,
        config: &BrowserDriverConfig,
    }
}

/// Manages browser workspaces backed by directory handles.
#[derive(Default)]
pub struct BrowserDriver {
    mounts: RefCell<HashMap<String, Rc<dyn AsyncStorage>>>,
}

impl std::fmt::Debug for BrowserDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserDriver").finish_non_exhaustive()
    }
}

#[async_trait(?Send)]
impl Driver for BrowserDriver {
    async fn mount(
        &self,
        config: &MountConfig,
    ) -> Result<Box<dyn Mount>, MountError> {
        let storage: Rc<dyn AsyncStorage> =
            Rc::from(config.storage.connect_async().await.map_err(
                |error| MountError::CannotMountAtPath {
                    path: config.path.clone(),
                    message: error.to_string(),
                },
            )?);
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
                    message: error.to_string(),
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
    storage: Rc<dyn AsyncStorage>,
}

#[async_trait(?Send)]
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
                message: error.to_string(),
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
                message: error.to_string(),
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

pub struct FileSystemAccessStorage {
    handle: JsValue,
    connected: Cell<bool>,
}

impl FileSystemAccessStorage {
    pub fn new(handle: JsValue) -> Self {
        Self {
            handle,
            connected: Cell::new(false),
        }
    }
}

#[async_trait(?Send)]
impl AsyncStorage for FileSystemAccessStorage {
    async fn connect(
        &self,
        request_permission: bool,
    ) -> Result<bool, StorageError> {
        let result: Result<bool, JsValue> = async {
            let options = Object::new();
            Reflect::set(
                &options,
                &JsValue::from_str("mode"),
                &JsValue::from_str("readwrite"),
            )?;
            let mut permission = call_async(
                &self.handle,
                "queryPermission",
                &Array::of1(&options),
            )
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
            let connected =
                permission.as_string().as_deref() == Some("granted");
            self.connected.set(connected);
            Ok(connected)
        }
        .await;
        result.map_err(storage_error)
    }

    async fn disconnect(&self) -> Result<(), StorageError> {
        self.connected.set(false);
        Ok(())
    }

    async fn read(&self, path: StoragePath) -> Result<Vec<u8>, StorageError> {
        let result: Result<Vec<u8>, JsValue> = async {
            self.ensure_connected()?;
            let file_handle = self.file_handle(&path, false).await?;
            let file =
                call_async(&file_handle, "getFile", &Array::new()).await?;
            let buffer =
                call_async(&file, "arrayBuffer", &Array::new()).await?;
            Ok(js_sys::Uint8Array::new(&buffer).to_vec())
        }
        .await;
        result.map_err(storage_error)
    }

    async fn write(
        &self,
        path: StoragePath,
        bytes: &[u8],
    ) -> Result<(), StorageError> {
        let result: Result<(), JsValue> = async {
            self.ensure_connected()?;
            let file_handle = self.file_handle(&path, true).await?;
            let writable =
                call_async(&file_handle, "createWritable", &Array::new())
                    .await?;
            let data = js_sys::Uint8Array::from(bytes);
            let result = async {
                call_async(&writable, "write", &Array::of1(&data)).await?;
                call_async(&writable, "close", &Array::new()).await
            }
            .await;
            if let Err(error) = result {
                let _ = call_async(&writable, "abort", &Array::new()).await;
                return Err(error);
            }
            Ok(())
        }
        .await;
        result.map_err(storage_error)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn is_connected(&self) -> bool {
        self.connected.get()
    }
}

impl FileSystemAccessStorage {
    async fn file_handle(
        &self,
        path: &StoragePath,
        create: bool,
    ) -> Result<JsValue, JsValue> {
        let mut parts = path.as_path().iter().peekable();
        let mut directory = self.handle.clone();
        let mut name = None;
        while let Some(part) = parts.next() {
            let part = part
                .to_str()
                .ok_or_else(|| JsValue::from_str("path is not UTF-8"))?;
            if parts.peek().is_none() {
                name = Some(part);
                break;
            }
            directory = call_async(
                &directory,
                "getDirectoryHandle",
                &Array::of1(&JsValue::from_str(part)),
            )
            .await?;
        }
        let name =
            name.ok_or_else(|| JsValue::from_str("path must name a file"))?;
        let options = Object::new();
        Reflect::set(
            &options,
            &JsValue::from_str("create"),
            &JsValue::from_bool(create),
        )?;
        call_async(
            &directory,
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
pub struct BrowserStorageConfig;

#[async_trait(?Send)]
#[typetag::serde(name = "browser-directory")]
impl StorageConfig for BrowserStorageConfig {
    fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        Err(ConnectStorageError::Failed {
            kind: self.kind(),
            message: "browser storage requires asynchronous connection"
                .to_owned(),
        })
    }

    async fn connect_async(
        &self,
    ) -> Result<Box<dyn AsyncStorage>, ConnectStorageError> {
        let result: Result<JsValue, JsValue> = async {
            let options = Object::new();
            Reflect::set(
                &options,
                &JsValue::from_str("mode"),
                &JsValue::from_str("readwrite"),
            )?;
            call_async(
                &js_sys::global(),
                "showDirectoryPicker",
                &Array::of1(&options),
            )
            .await
        }
        .await;
        result
            .map(|handle| {
                Box::new(FileSystemAccessStorage::new(handle))
                    as Box<dyn AsyncStorage>
            })
            .map_err(|error| ConnectStorageError::Failed {
                kind: self.kind(),
                message: js_message(&error),
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

fn js_message(error: &JsValue) -> String {
    Reflect::get(error, &JsValue::from_str("message"))
        .ok()
        .and_then(|message| message.as_string())
        .unwrap_or_else(|| format!("{error:?}"))
}

fn storage_error(error: JsValue) -> StorageError {
    StorageError {
        message: js_message(&error),
        not_found: Reflect::get(&error, &JsValue::from_str("name"))
            .ok()
            .and_then(|name| name.as_string())
            .as_deref()
            == Some("NotFoundError"),
    }
}
