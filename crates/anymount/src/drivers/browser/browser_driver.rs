//! Implements the browser workspace driver using user-authorized directory handles.

#[path = "browser_driver_api.rs"]
mod browser_driver_api;

pub use browser_driver_api::BrowserDriverApi;

use crate::domain::{
    AsyncStorage, AsyncStorageError, ConnectMountError, ConnectStorageError,
    DisconnectMountError, Driver, DriverConfig, Mount, MountConfig, MountError,
    Storage, StorageConfig, StorageEntry, StorageFuture, StoragePath,
    UnregisterMountError,
};
use async_trait::async_trait;
use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
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

/// Stores files in a browser-authorized directory.
pub struct FileSystemAccessStorage {
    handle: JsValue,
    connected: Cell<bool>,
}

impl FileSystemAccessStorage {
    /// Creates a storage provider from a directory handle.
    pub fn new(handle: JsValue) -> Self {
        Self {
            handle,
            connected: Cell::new(false),
        }
    }
}

impl std::fmt::Debug for FileSystemAccessStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSystemAccessStorage")
            .finish_non_exhaustive()
    }
}

#[async_trait(?Send)]
impl AsyncStorage for FileSystemAccessStorage {
    async fn connect(
        &self,
        request_permission: bool,
    ) -> Result<bool, AsyncStorageError> {
        self.connected.set(false);
        let options = Object::new();
        Reflect::set(
            &options,
            &JsValue::from_str("mode"),
            &JsValue::from_str("readwrite"),
        )
        .map_err(storage_error)?;
        let mut permission =
            call_async(&self.handle, "queryPermission", &Array::of1(&options))
                .await
                .map_err(storage_error)?;
        if permission.as_string().as_deref() != Some("granted")
            && request_permission
        {
            permission = call_async(
                &self.handle,
                "requestPermission",
                &Array::of1(&options),
            )
            .await
            .map_err(storage_error)?;
        }
        let connected = permission.as_string().as_deref() == Some("granted");
        self.connected.set(connected);
        Ok(connected)
    }

    async fn disconnect(&self) -> Result<(), AsyncStorageError> {
        self.connected.set(false);
        Ok(())
    }

    async fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<Vec<StorageEntry>, AsyncStorageError> {
        self.ensure_connected()?;
        let parts: Vec<_> = path.as_path().iter().collect();
        let directory = self
            .directory_handle(&parts)
            .await
            .map_err(|error| self.io_error(error))?;
        let iterator = call(&directory, "values", &Array::new())
            .map_err(|error| self.io_error(error))?;
        let mut entries = Vec::new();
        loop {
            let next = call_async(&iterator, "next", &Array::new())
                .await
                .map_err(|error| self.io_error(error))?;
            let done = Reflect::get(&next, &JsValue::from_str("done"))
                .map_err(|error| self.io_error(error))?;
            if done.as_bool().unwrap_or(false) {
                break;
            }
            let entry = Reflect::get(&next, &JsValue::from_str("value"))
                .map_err(|error| self.io_error(error))?;
            let name = Reflect::get(&entry, &JsValue::from_str("name"))
                .map_err(|error| self.io_error(error))?
                .as_string()
                .ok_or_else(|| AsyncStorageError::Failed {
                    message: "directory entry has no name".to_owned(),
                })?;
            let kind = Reflect::get(&entry, &JsValue::from_str("kind"))
                .map_err(|error| self.io_error(error))?
                .as_string()
                .ok_or_else(|| AsyncStorageError::Failed {
                    message: "directory entry has no kind".to_owned(),
                })?;
            let is_directory = match kind.as_str() {
                "directory" => true,
                "file" => false,
                _ => {
                    return Err(AsyncStorageError::Failed {
                        message: format!(
                            "unknown directory entry kind: {kind}"
                        ),
                    });
                }
            };
            entries.push(StorageEntry { name, is_directory });
        }
        Ok(entries)
    }

    async fn read(
        &self,
        path: StoragePath,
    ) -> Result<Vec<u8>, AsyncStorageError> {
        self.ensure_connected()?;
        let file_handle = self
            .file_handle(path, false)
            .await
            .map_err(|error| self.io_error(error))?;
        let file = call_async(&file_handle, "getFile", &Array::new())
            .await
            .map_err(|error| self.io_error(error))?;
        let buffer = call_async(&file, "arrayBuffer", &Array::new())
            .await
            .map_err(|error| self.io_error(error))?;
        Ok(Uint8Array::new(&buffer).to_vec())
    }

    async fn write(
        &self,
        path: StoragePath,
        value: &[u8],
    ) -> Result<(), AsyncStorageError> {
        self.ensure_connected()?;
        let file_handle = self
            .file_handle(path, true)
            .await
            .map_err(|error| self.io_error(error))?;
        let writable =
            call_async(&file_handle, "createWritable", &Array::new())
                .await
                .map_err(|error| self.io_error(error))?;
        let result = async {
            let bytes = Uint8Array::from(value);
            call_async(&writable, "write", &Array::of1(&bytes)).await?;
            call_async(&writable, "close", &Array::new()).await
        }
        .await;
        if let Err(error) = result {
            let _ = call_async(&writable, "abort", &Array::new()).await;
            return Err(self.io_error(error));
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn is_connected(&self) -> bool {
        self.connected.get()
    }
}

impl FileSystemAccessStorage {
    fn io_error(&self, error: JsValue) -> AsyncStorageError {
        let error = storage_error(error);
        if matches!(error, AsyncStorageError::PermissionDenied) {
            self.connected.set(false);
        }
        error
    }

    async fn directory_handle(
        &self,
        directories: &[&std::ffi::OsStr],
    ) -> Result<JsValue, JsValue> {
        let mut parent = self.handle.clone();
        for directory in directories {
            let name = directory
                .to_str()
                .ok_or_else(|| JsValue::from_str("path is not UTF-8"))?;
            parent = call_async(
                &parent,
                "getDirectoryHandle",
                &Array::of1(&JsValue::from_str(name)),
            )
            .await?;
        }
        Ok(parent)
    }

    async fn file_handle(
        &self,
        path: StoragePath,
        create: bool,
    ) -> Result<JsValue, JsValue> {
        let parts: Vec<_> = path.as_path().iter().collect();
        let (name, directories) = parts
            .split_last()
            .ok_or_else(|| JsValue::from_str("file path is empty"))?;
        let parent = self.directory_handle(directories).await?;
        let name = name
            .to_str()
            .ok_or_else(|| JsValue::from_str("path is not UTF-8"))?;
        let options = Object::new();
        Reflect::set(
            &options,
            &JsValue::from_str("create"),
            &JsValue::from_bool(create),
        )?;
        call_async(
            &parent,
            "getFileHandle",
            &Array::of2(&JsValue::from_str(name), &options),
        )
        .await
    }

    fn ensure_connected(&self) -> Result<(), AsyncStorageError> {
        if self.is_connected() {
            Ok(())
        } else {
            Err(AsyncStorageError::Disconnected)
        }
    }
}

/// Selects a browser directory as storage.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct BrowserStorageConfig;

#[typetag::serde(name = "browser")]
impl StorageConfig for BrowserStorageConfig {
    fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        Err(ConnectStorageError::Failed {
            kind: self.kind(),
            message: "browser storage requires an asynchronous connection"
                .to_owned(),
        })
    }

    fn connect_async(
        &self,
    ) -> StorageFuture<'_, Result<Box<dyn AsyncStorage>, ConnectStorageError>>
    {
        Box::pin(async {
            let options = Object::new();
            Reflect::set(
                &options,
                &JsValue::from_str("mode"),
                &JsValue::from_str("readwrite"),
            )
            .map_err(|error| ConnectStorageError::Failed {
                kind: self.kind(),
                message: js_message(&error),
            })?;
            let handle = call_async(
                &js_sys::global(),
                "showDirectoryPicker",
                &Array::of1(&options),
            )
            .await
            .map_err(|error| ConnectStorageError::Failed {
                kind: self.kind(),
                message: js_message(&error),
            })?;
            Ok(Box::new(FileSystemAccessStorage::new(handle))
                as Box<dyn AsyncStorage>)
        })
    }
}

async fn call_async(
    receiver: &JsValue,
    method: &str,
    args: &Array,
) -> Result<JsValue, JsValue> {
    let promise: Promise =
        call(receiver, method, args)?.dyn_into().map_err(|_| {
            JsValue::from_str("browser API did not return a promise")
        })?;
    JsFuture::from(promise).await
}

fn call(
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
    function.apply(receiver, args)
}

fn storage_error(error: JsValue) -> AsyncStorageError {
    match Reflect::get(&error, &JsValue::from_str("name"))
        .ok()
        .and_then(|v| v.as_string())
        .as_deref()
    {
        Some("NotFoundError") => AsyncStorageError::NotFound,
        Some("NotAllowedError") => AsyncStorageError::PermissionDenied,
        _ => AsyncStorageError::Failed {
            message: js_message(&error),
        },
    }
}

fn js_message(error: &JsValue) -> String {
    Reflect::get(error, &JsValue::from_str("message"))
        .ok()
        .and_then(|message| message.as_string())
        .unwrap_or_else(|| format!("{error:?}"))
}
