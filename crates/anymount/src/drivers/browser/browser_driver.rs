//! Implements the browser workspace driver using user-authorized directory handles.

#[path = "browser_driver_api.rs"]
mod browser_driver_api;

pub use browser_driver_api::BrowserDriverApi;

use crate::domain::{
    ConnectMountError, ConnectPermissionError, ConnectStorageError, DirEntry,
    DisconnectMountError, DisconnectStorageError, Driver, DriverConfig, Mount,
    MountConfig, MountError, ReadDirError, ReadFileAtError, Storage,
    StorageConfig, StoragePath, UnregisterMountError, WriteFileError,
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
    mounts: RefCell<HashMap<String, Rc<dyn Storage>>>,
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
        let storage: Rc<dyn Storage> =
            Rc::from(config.storage.connect().await.map_err(|error| {
                MountError::CannotMountAtPath {
                    path: config.path.clone(),
                    message: error.to_string(),
                }
            })?);
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
    storage: Rc<dyn Storage>,
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
    connected: Rc<Cell<bool>>,
}

struct FileSystemAccessDirEntry {
    name: String,
    is_directory: bool,
}

impl DirEntry for FileSystemAccessDirEntry {
    fn file_name(&self) -> String {
        self.name.clone()
    }

    fn is_dir(&self) -> bool {
        self.is_directory
    }

    fn size(&self) -> Option<u64> {
        None
    }

    fn accessed(&self) -> Option<std::time::SystemTime> {
        None
    }
}

impl FileSystemAccessStorage {
    /// Creates a storage provider from a directory handle.
    pub fn new(handle: JsValue) -> Self {
        Self {
            handle,
            connected: Rc::new(Cell::new(false)),
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
impl Storage for FileSystemAccessStorage {
    async fn connect(
        &self,
        request_permission: bool,
    ) -> Result<bool, ConnectPermissionError> {
        self.connected.set(false);
        let options = Object::new();
        Reflect::set(
            &options,
            &JsValue::from_str("mode"),
            &JsValue::from_str("readwrite"),
        )
        .map_err(connect_permission_error)?;
        let mut permission =
            call_async(&self.handle, "queryPermission", &Array::of1(&options))
                .await
                .map_err(connect_permission_error)?;
        if permission.as_string().as_deref() != Some("granted")
            && request_permission
        {
            permission = call_async(
                &self.handle,
                "requestPermission",
                &Array::of1(&options),
            )
            .await
            .map_err(connect_permission_error)?;
        }
        let connected = permission.as_string().as_deref() == Some("granted");
        self.connected.set(connected);
        Ok(connected)
    }

    async fn disconnect(&self) -> Result<(), DisconnectStorageError> {
        self.connected.set(false);
        Ok(())
    }

    async fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<crate::domain::DirStream, ReadDirError> {
        if !self.is_connected() {
            return Err(ReadDirError::Disconnected);
        }
        let parts: Vec<_> = path.as_path().iter().collect();
        let directory = self
            .directory_handle(&parts)
            .await
            .map_err(|error| self.read_dir_error(error))?;
        let iterator = call(&directory, "values", &Array::new())
            .map_err(|error| self.read_dir_error(error))?;
        let storage = Self {
            handle: self.handle.clone(),
            connected: self.connected.clone(),
        };
        Ok(Box::pin(futures::stream::unfold(
            Some((iterator, storage)),
            |state| async move {
                let (iterator, storage) = state?;
                let result = async {
                    let next = call_async(&iterator, "next", &Array::new())
                        .await
                        .map_err(|error| storage.read_dir_error(error))?;
                    let done = Reflect::get(&next, &JsValue::from_str("done"))
                        .map_err(|error| storage.read_dir_error(error))?;
                    if done.as_bool().unwrap_or(false) {
                        return Ok(None);
                    }
                    let entry =
                        Reflect::get(&next, &JsValue::from_str("value"))
                            .map_err(|error| storage.read_dir_error(error))?;
                    let name = Reflect::get(&entry, &JsValue::from_str("name"))
                        .map_err(|error| storage.read_dir_error(error))?
                        .as_string()
                        .ok_or_else(|| ReadDirError::Unknown {
                            message: "directory entry has no name".to_owned(),
                        })?;
                    let kind = Reflect::get(&entry, &JsValue::from_str("kind"))
                        .map_err(|error| storage.read_dir_error(error))?
                        .as_string()
                        .ok_or_else(|| ReadDirError::Unknown {
                            message: "directory entry has no kind".to_owned(),
                        })?;
                    let is_directory = match kind.as_str() {
                        "directory" => true,
                        "file" => false,
                        _ => {
                            return Err(ReadDirError::Unknown {
                                message: format!(
                                    "unknown directory entry kind: {kind}"
                                ),
                            });
                        }
                    };
                    Ok(Some(Box::new(FileSystemAccessDirEntry {
                        name,
                        is_directory,
                    }) as Box<dyn DirEntry>))
                }
                .await;
                match result {
                    Ok(Some(entry)) => {
                        Some((Ok(entry), Some((iterator, storage))))
                    }
                    Ok(None) => None,
                    Err(error) => Some((Err(error), None)),
                }
            },
        )))
    }

    async fn read_file_at(
        &self,
        path: StoragePath,
        writer: &mut dyn crate::domain::WriteAt,
        range: std::ops::Range<u64>,
    ) -> Result<u64, ReadFileAtError> {
        if !self.is_connected() {
            return Err(ReadFileAtError::Disconnected);
        }
        if range.start > range.end || range.end > 9_007_199_254_740_991 {
            return Err(ReadFileAtError::RangeNotSatisfiable);
        }
        let file_handle = self
            .file_handle(path, false)
            .await
            .map_err(|error| self.read_file_error(error))?;
        let file = call_async(&file_handle, "getFile", &Array::new())
            .await
            .map_err(|error| self.read_file_error(error))?;
        let mut offset = range.start;
        while offset < range.end {
            let end = offset
                + (range.end - offset)
                    .min(crate::domain::MAX_READ_FILE_AT_RANGE_SIZE as u64);
            let slice = call(
                &file,
                "slice",
                &Array::of2(
                    &JsValue::from_f64(offset as f64),
                    &JsValue::from_f64(end as f64),
                ),
            )
            .map_err(|error| self.read_file_error(error))?;
            let buffer = call_async(&slice, "arrayBuffer", &Array::new())
                .await
                .map_err(|error| self.read_file_error(error))?;
            let bytes = Uint8Array::new(&buffer).to_vec();
            if bytes.is_empty() {
                break;
            }
            if bytes.len() as u64 > end - offset {
                return Err(ReadFileAtError::Unknown {
                    message: "provider returned too much data".into(),
                });
            }
            writer.write_at(&bytes, offset)?;
            offset += bytes.len() as u64;
        }
        Ok(offset - range.start)
    }

    async fn write_file(
        &self,
        path: StoragePath,
        value: &[u8],
    ) -> Result<(), WriteFileError> {
        if !self.is_connected() {
            return Err(WriteFileError::Disconnected);
        }
        let file_handle = self
            .file_handle(path, true)
            .await
            .map_err(|error| self.write_error(error))?;
        let writable =
            call_async(&file_handle, "createWritable", &Array::new())
                .await
                .map_err(|error| self.write_error(error))?;
        let result = async {
            let bytes = Uint8Array::from(value);
            call_async(&writable, "write", &Array::of1(&bytes)).await?;
            call_async(&writable, "close", &Array::new()).await
        }
        .await;
        if let Err(error) = result {
            let _ = call_async(&writable, "abort", &Array::new()).await;
            return Err(self.write_error(error));
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
    fn error_name(&self, error: &JsValue) -> Option<String> {
        let name = Reflect::get(error, &JsValue::from_str("name"))
            .ok()
            .and_then(|value| value.as_string());
        if name.as_deref() == Some("NotAllowedError") {
            self.connected.set(false);
        }
        name
    }

    fn read_dir_error(&self, error: JsValue) -> ReadDirError {
        match self.error_name(&error).as_deref() {
            Some("NotFoundError") => ReadDirError::NotFound,
            Some("NotAllowedError") => ReadDirError::PermissionDenied,
            Some("TypeMismatchError") => ReadDirError::NotADirectory,
            _ => ReadDirError::Unknown {
                message: js_message(&error),
            },
        }
    }

    fn read_file_error(&self, error: JsValue) -> ReadFileAtError {
        match self.error_name(&error).as_deref() {
            Some("NotFoundError") => ReadFileAtError::NotFound,
            Some("NotAllowedError") => ReadFileAtError::PermissionDenied,
            Some("TypeMismatchError") => ReadFileAtError::NotAFile,
            _ => ReadFileAtError::Unknown {
                message: js_message(&error),
            },
        }
    }

    fn write_error(&self, error: JsValue) -> WriteFileError {
        match self.error_name(&error).as_deref() {
            Some("NotFoundError") => WriteFileError::NotFound,
            Some("NotAllowedError") => WriteFileError::PermissionDenied,
            Some("TypeMismatchError") => WriteFileError::NotAFile,
            _ => WriteFileError::Failed {
                message: js_message(&error),
            },
        }
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
}

/// Selects a browser directory as storage.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct BrowserStorageConfig;

#[typetag::serde(name = "browser")]
#[async_trait(?Send)]
impl StorageConfig for BrowserStorageConfig {
    fn clone_config(&self) -> Box<dyn StorageConfig> {
        Box::new(self.clone())
    }
    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
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
        Ok(Box::new(FileSystemAccessStorage::new(handle)) as Box<dyn Storage>)
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

fn connect_permission_error(error: JsValue) -> ConnectPermissionError {
    match Reflect::get(&error, &JsValue::from_str("name"))
        .ok()
        .and_then(|value| value.as_string())
        .as_deref()
    {
        Some("NotAllowedError") => ConnectPermissionError::PermissionDenied,
        _ => ConnectPermissionError::Failed {
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::domain::{Config, MountConfig, StorageConfig};
    use crate::drivers::{
        browser::{BrowserDriver, BrowserDriverConfig},
        get_default_driver_config,
    };
    use js_sys::{Function, Reflect};
    use wasm_bindgen::JsValue;
    use wasm_bindgen_test::wasm_bindgen_test;

    use crate::test_utils::mocks::BytesWriter as TestWriter;

    #[wasm_bindgen_test]
    fn browser_driver_is_public_and_implements_driver() {
        let driver = BrowserDriver::default();
        let _: &dyn crate::domain::Driver = &driver;
    }

    #[wasm_bindgen_test]
    fn browser_driver_is_the_default() {
        let config =
            get_default_driver_config().expect("default driver is registered");

        assert_eq!(config.kind(), std::any::type_name::<BrowserDriverConfig>());
    }

    #[wasm_bindgen_test]
    fn shared_read_helpers_are_available() {
        let config: Box<dyn StorageConfig> =
            serde_json::from_str(r#"{"type":"browser"}"#)
                .expect("browser storage configuration is registered");
        let _ = config;
        let _read_file_at = crate::read_file_at::<dyn crate::Storage>;
    }

    #[wasm_bindgen_test]
    async fn shared_mount_uses_registered_browser_driver_and_storage_config() {
        let storage: Box<dyn StorageConfig> =
            serde_json::from_str(r#"{"type":"browser"}"#)
                .expect("browser storage configuration is registered");
        let global = js_sys::global();
        let picker = Function::new_no_args("return Promise.resolve({})");
        Reflect::set(
            &global,
            &JsValue::from_str("showDirectoryPicker"),
            &picker,
        )
        .expect("install directory picker");

        let config = Config {
            driver: None,
            mount: MountConfig {
                name: "workspace".to_owned(),
                path: PathBuf::from("workspace"),
                storage,
            },
        };
        let mount = crate::mount(config).await.expect("mount workspace");

        assert_eq!(mount.name(), "workspace");
        assert!(!mount.is_connected());
        Reflect::delete_property(
            &global,
            &JsValue::from_str("showDirectoryPicker"),
        )
        .expect("remove directory picker");
    }
    #[wasm_bindgen_test]
    async fn browser_storage_round_trips_bytes_and_reconnects() {
        use crate::domain::{ReadFileAtError, StoragePath};
        let create_handle = Function::new_no_args(
            r#"
            let data;
            let permission = 'prompt';
            return {
                queryPermission: async () => permission,
                requestPermission: async () => { permission = 'granted'; return permission; },
                revoke: () => { permission = 'denied'; },
                getFileHandle: async (name, options) => {
                    if (permission !== 'granted') throw new DOMException('denied', 'NotAllowedError');
                    if (data === undefined && !options.create) throw new DOMException('missing', 'NotFoundError');
                    return {
                        getFile: async () => ({ slice: (start, end) => ({ arrayBuffer: async () => data.slice(start, end).buffer }) }),
                        createWritable: async () => ({
                            write: async bytes => { data = new Uint8Array(bytes); },
                            close: async () => {}, abort: async () => {}
                        })
                    };
                }
            };
        "#,
        );
        let handle =
            create_handle.call0(&JsValue::NULL).expect("create handle");
        let picker = Function::new_with_args(
            "handle",
            "return () => Promise.resolve(handle)",
        )
        .call1(&JsValue::NULL, &handle)
        .expect("create picker");
        let global = js_sys::global();
        Reflect::set(
            &global,
            &JsValue::from_str("showDirectoryPicker"),
            &picker,
        )
        .expect("install picker");
        let config: Box<dyn StorageConfig> =
            serde_json::from_str(r#"{"type":"browser"}"#).expect("config");
        let storage = config.connect().await.expect("select directory");
        let path =
            StoragePath::try_from(PathBuf::from("note.bin")).expect("path");
        let mut writer = TestWriter::default();
        assert!(!storage.connect(false).await.expect("query"));
        assert!(matches!(
            storage
                .read_file_at(path.clone(), &mut writer, 0..65536)
                .await,
            Err(ReadFileAtError::Disconnected)
        ));
        assert!(storage.connect(true).await.expect("request"));
        assert!(matches!(
            storage
                .read_file_at(path.clone(), &mut writer, 0..65536)
                .await,
            Err(ReadFileAtError::NotFound)
        ));
        storage
            .write_file(path.clone(), &[0, 255, 42])
            .await
            .expect("write");
        assert_eq!(
            storage
                .read_file_at(path.clone(), &mut writer, 1..2)
                .await
                .expect("bounded read"),
            1
        );
        assert_eq!(writer.0, vec![255]);
        writer.0.clear();
        assert_eq!(
            storage
                .read_file_at(path.clone(), &mut writer, 3..4)
                .await
                .expect("EOF"),
            0
        );
        assert!(
            storage
                .read_file_at(
                    path.clone(),
                    &mut writer,
                    9_007_199_254_740_991..9_007_199_254_740_992
                )
                .await
                .is_err()
        );
        assert_eq!(
            storage
                .read_file_at(path.clone(), &mut writer, 0..65537)
                .await
                .expect("read across chunks"),
            3
        );
        assert_eq!(writer.0, vec![0, 255, 42]);
        writer.0.clear();
        storage.disconnect().await.expect("disconnect");
        assert!(storage.connect(false).await.expect("reconnect"));
        assert_eq!(
            storage
                .read_file_at(path.clone(), &mut writer, 0..65536)
                .await
                .expect("read"),
            3
        );
        assert_eq!(writer.0, vec![0, 255, 42]);
        Function::new_with_args("handle", "handle.revoke()")
            .call1(&JsValue::NULL, &handle)
            .expect("revoke permission");
        assert!(matches!(
            storage
                .read_file_at(path.clone(), &mut writer, 0..65536)
                .await,
            Err(ReadFileAtError::PermissionDenied)
        ));
        assert!(!storage.is_connected());
        assert!(storage.connect(true).await.expect("request again"));
        writer.0.clear();
        assert_eq!(
            storage
                .read_file_at(path, &mut writer, 0..65536)
                .await
                .expect("read again"),
            3
        );
        assert_eq!(writer.0, vec![0, 255, 42]);
        Reflect::delete_property(
            &global,
            &JsValue::from_str("showDirectoryPicker"),
        )
        .expect("remove picker");
    }

    #[wasm_bindgen_test]
    async fn browser_storage_lists_root_and_nested_directories() {
        use super::FileSystemAccessStorage;
        use crate::domain::{ReadDirError, Storage, StoragePath};
        use futures::StreamExt;

        let handle = Function::new_no_args(
            r#"
            const listing = entries => ({
                values: () => {
                    let index = 0;
                    return { next: async () => index < entries.length
                        ? { value: entries[index++], done: false }
                        : { done: true } };
                }
            });
            const child = listing([{ name: 'note.txt', kind: 'file' }]);
            return Object.assign(listing([{ name: 'notes', kind: 'directory' }]), {
                queryPermission: async () => 'granted',
                getDirectoryHandle: async name => {
                    if (name === 'notes') return child;
                    throw new DOMException('missing', 'NotFoundError');
                }
            });
        "#,
        ).call0(&JsValue::NULL).expect("create handle");
        let storage = FileSystemAccessStorage::new(handle);
        assert!(matches!(
            storage.read_dir(StoragePath::root()).await,
            Err(ReadDirError::Disconnected)
        ));
        assert!(storage.connect(false).await.expect("connect"));
        let mut root =
            storage.read_dir(StoragePath::root()).await.expect("root");
        let entry = root.next().await.expect("entry").expect("valid entry");
        assert_eq!(entry.file_name(), "notes");
        assert!(entry.is_dir());
        assert_eq!(entry.size(), None);
        assert_eq!(entry.accessed(), None);
        assert!(root.next().await.is_none());
        let path = StoragePath::try_from(PathBuf::from("notes")).expect("path");
        let mut nested = storage.read_dir(path).await.expect("nested");
        let entry = nested.next().await.expect("entry").expect("valid entry");
        assert_eq!(entry.file_name(), "note.txt");
        assert!(!entry.is_dir());
        assert_eq!(entry.size(), None);
        assert_eq!(entry.accessed(), None);
        assert!(nested.next().await.is_none());
        let missing =
            StoragePath::try_from(PathBuf::from("missing")).expect("path");
        assert!(matches!(
            storage.read_dir(missing).await,
            Err(ReadDirError::NotFound)
        ));
    }

    #[wasm_bindgen_test]
    async fn browser_directory_stream_is_lazy_and_reports_permission_errors() {
        use super::FileSystemAccessStorage;
        use crate::domain::{ReadDirError, Storage, StoragePath};
        use futures::StreamExt;

        let handle = Function::new_no_args(
            r#"
            const handle = { calls: 0, queryPermission: async () => 'granted' };
            handle.values = () => ({ next: async () => {
                handle.calls++;
                if (handle.calls === 1) return { value: { name: 'first', kind: 'file' }, done: false };
                throw new DOMException('denied', 'NotAllowedError');
            }});
            return handle;
            "#,
        ).call0(&JsValue::NULL).expect("create handle");
        let storage = FileSystemAccessStorage::new(handle.clone());
        assert!(storage.connect(false).await.expect("connect"));
        let mut entries = storage
            .read_dir(StoragePath::root())
            .await
            .expect("listing");
        let calls = || {
            Reflect::get(&handle, &JsValue::from_str("calls"))
                .expect("calls")
                .as_f64()
        };
        assert_eq!(calls(), Some(0.0));
        assert_eq!(
            entries
                .next()
                .await
                .expect("entry")
                .expect("valid entry")
                .file_name(),
            "first"
        );
        assert_eq!(calls(), Some(1.0));
        assert!(matches!(
            entries.next().await,
            Some(Err(ReadDirError::PermissionDenied))
        ));
        assert!(!storage.is_connected());
        assert!(entries.next().await.is_none());
        assert_eq!(calls(), Some(2.0));
    }

    #[wasm_bindgen_test]
    async fn browser_write_permission_failure_disconnects_storage() {
        use super::FileSystemAccessStorage;
        use crate::domain::{Storage, StoragePath, WriteFileError};

        let handle = Function::new_no_args(
            "return { queryPermission: async () => 'granted', getFileHandle: async () => { throw new DOMException('denied', 'NotAllowedError'); } }",
        ).call0(&JsValue::NULL).expect("create handle");
        let storage = FileSystemAccessStorage::new(handle);
        let path =
            StoragePath::try_from(PathBuf::from("note.bin")).expect("path");
        assert!(matches!(
            storage.write_file(path.clone(), &[]).await,
            Err(WriteFileError::Disconnected)
        ));
        assert!(storage.connect(false).await.expect("connect"));
        assert!(matches!(
            storage.write_file(path.clone(), &[]).await,
            Err(WriteFileError::PermissionDenied)
        ));
        assert!(!storage.is_connected());
        assert!(matches!(
            storage.write_file(path, &[]).await,
            Err(WriteFileError::Disconnected)
        ));
    }

    #[wasm_bindgen_test]
    async fn browser_api_restores_saved_handle_with_a_new_driver() {
        use super::BrowserDriverApi;
        let handle = Function::new_no_args(
            r#"
            let data = new Uint8Array();
            return {
                queryPermission: async () => 'granted',
                getFileHandle: async () => ({
                    getFile: async () => ({ slice: (start, end) => ({ arrayBuffer: async () => data.slice(start, end).buffer }) }),
                    createWritable: async () => ({
                        write: async value => { data = new Uint8Array(value); },
                        close: async () => {}, abort: async () => {}
                    })
                })
            };
        "#,
        )
        .call0(&JsValue::NULL)
        .expect("create handle");
        let picker = Function::new_with_args(
            "handle",
            "return () => Promise.resolve(handle)",
        )
        .call1(&JsValue::NULL, &handle)
        .expect("create picker");
        let global = js_sys::global();
        Reflect::set(
            &global,
            &JsValue::from_str("showDirectoryPicker"),
            &picker,
        )
        .expect("install picker");
        let first = BrowserDriverApi::new();
        first
            .mount_workspace("workspace".into(), "workspace".into())
            .await
            .expect("mount");
        first
            .write_text(
                "workspace".into(),
                "note.txt".into(),
                "saved note".into(),
            )
            .await
            .expect("write");
        let saved = first.directory_handle("workspace".into()).expect("handle");
        let restored = BrowserDriverApi::new();
        assert!(
            restored
                .restore_workspace(
                    "workspace".into(),
                    "workspace".into(),
                    saved
                )
                .await
                .expect("restore")
        );
        assert_eq!(
            restored
                .read_text("workspace".into(), "note.txt".into())
                .await
                .expect("read"),
            "saved note"
        );
        Reflect::delete_property(
            &global,
            &JsValue::from_str("showDirectoryPicker"),
        )
        .expect("remove picker");
    }

    #[wasm_bindgen_test]
    async fn failed_workspace_replacement_keeps_existing_storage() {
        use super::BrowserDriverApi;

        let picker = Function::new_no_args(
            r#"
            let data = new Uint8Array();
            let selected = 0;
            const first = {
                queryPermission: async () => 'granted',
                getFileHandle: async () => ({
                    getFile: async () => ({ slice: (start, end) => ({ arrayBuffer: async () => data.slice(start, end).buffer }) }),
                    createWritable: async () => ({
                        write: async value => { data = new Uint8Array(value); },
                        close: async () => {}, abort: async () => {}
                    })
                })
            };
            const second = { queryPermission: async () => 'prompt' };
            return () => Promise.resolve(selected++ === 0 ? first : second);
        "#,
        ).call0(&JsValue::NULL).expect("create picker");
        let global = js_sys::global();
        Reflect::set(
            &global,
            &JsValue::from_str("showDirectoryPicker"),
            &picker,
        )
        .expect("install picker");
        let driver = BrowserDriverApi::new();
        driver
            .mount_workspace("workspace".into(), "workspace".into())
            .await
            .expect("mount");
        driver
            .write_text(
                "workspace".into(),
                "note.txt".into(),
                "saved note".into(),
            )
            .await
            .expect("write");

        assert!(
            driver
                .mount_workspace("workspace".into(), "workspace".into())
                .await
                .is_err()
        );
        let broken = Function::new_no_args("return { queryPermission: async () => { throw new Error('unavailable'); } }")
            .call0(&JsValue::NULL).expect("create broken handle");
        assert!(
            driver
                .restore_workspace(
                    "workspace".into(),
                    "workspace".into(),
                    broken
                )
                .await
                .is_err()
        );
        assert!(driver.is_connected("workspace".into()).expect("connected"));
        assert_eq!(
            driver
                .read_text("workspace".into(), "note.txt".into())
                .await
                .expect("read"),
            "saved note"
        );
        Reflect::delete_property(
            &global,
            &JsValue::from_str("showDirectoryPicker"),
        )
        .expect("remove picker");
    }

    #[wasm_bindgen_test]
    async fn failed_browser_write_aborts_the_stream() {
        use super::FileSystemAccessStorage;
        use crate::domain::{Storage, StoragePath};

        let handle = Function::new_no_args(
            r#"
            let handle = { aborts: 0 };
            handle.queryPermission = async () => 'granted';
            handle.getFileHandle = async () => ({
                createWritable: async () => ({
                    write: async () => {},
                    close: async () => { throw new Error('close failed'); },
                    abort: async () => { handle.aborts += 1; }
                })
            });
            return handle;
        "#,
        )
        .call0(&JsValue::NULL)
        .expect("create handle");
        let storage = FileSystemAccessStorage::new(handle.clone());
        assert!(storage.connect(false).await.expect("connect"));
        let path =
            StoragePath::try_from(PathBuf::from("note.bin")).expect("path");
        assert!(storage.write_file(path, &[1, 2]).await.is_err());
        assert_eq!(
            Reflect::get(&handle, &JsValue::from_str("aborts"))
                .expect("abort count")
                .as_f64(),
            Some(1.0)
        );
    }
}
