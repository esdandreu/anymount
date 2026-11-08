mod browser_driver;

pub use browser_driver::{
    BrowserDriver, BrowserDriverApi, BrowserDriverConfig, BrowserStorageConfig,
    FileSystemAccessStorage,
};

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
        assert!(config.connect().is_err());
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
        use crate::domain::{AsyncStorageError, StoragePath};
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
                        getFile: async () => ({ arrayBuffer: async () => data.buffer }),
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
        let storage = config.connect_async().await.expect("select directory");
        let path =
            StoragePath::try_from(PathBuf::from("note.bin")).expect("path");
        assert!(!storage.connect(false).await.expect("query"));
        assert!(matches!(
            storage.read(path.clone()).await,
            Err(AsyncStorageError::Disconnected)
        ));
        assert!(storage.connect(true).await.expect("request"));
        assert!(matches!(
            storage.read(path.clone()).await,
            Err(AsyncStorageError::NotFound)
        ));
        storage
            .write(path.clone(), &[0, 255, 42])
            .await
            .expect("write");
        storage.disconnect().await.expect("disconnect");
        assert!(storage.connect(false).await.expect("reconnect"));
        assert_eq!(
            storage.read(path.clone()).await.expect("read"),
            vec![0, 255, 42]
        );
        Function::new_with_args("handle", "handle.revoke()")
            .call1(&JsValue::NULL, &handle)
            .expect("revoke permission");
        assert!(matches!(
            storage.read(path.clone()).await,
            Err(AsyncStorageError::PermissionDenied)
        ));
        assert!(!storage.is_connected());
        assert!(storage.connect(true).await.expect("request again"));
        assert_eq!(
            storage.read(path).await.expect("read again"),
            vec![0, 255, 42]
        );
        Reflect::delete_property(
            &global,
            &JsValue::from_str("showDirectoryPicker"),
        )
        .expect("remove picker");
    }

    #[wasm_bindgen_test]
    async fn browser_storage_lists_root_and_nested_directories() {
        use super::FileSystemAccessStorage;
        use crate::domain::{
            AsyncStorage, AsyncStorageError, StorageEntry, StoragePath,
        };

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
            Err(AsyncStorageError::Disconnected)
        ));
        assert!(storage.connect(false).await.expect("connect"));
        assert_eq!(
            storage.read_dir(StoragePath::root()).await.expect("root"),
            vec![StorageEntry {
                name: "notes".into(),
                is_directory: true
            }]
        );
        let path = StoragePath::try_from(PathBuf::from("notes")).expect("path");
        assert_eq!(
            storage.read_dir(path).await.expect("nested"),
            vec![StorageEntry {
                name: "note.txt".into(),
                is_directory: false
            }]
        );
        let missing =
            StoragePath::try_from(PathBuf::from("missing")).expect("path");
        assert!(matches!(
            storage.read_dir(missing).await,
            Err(AsyncStorageError::NotFound)
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
                    getFile: async () => ({ arrayBuffer: async () => data.buffer }),
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
                    getFile: async () => ({ arrayBuffer: async () => data.buffer }),
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
        use crate::domain::{AsyncStorage, StoragePath};

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
        assert!(storage.write(path, &[1, 2]).await.is_err());
        assert_eq!(
            Reflect::get(&handle, &JsValue::from_str("aborts"))
                .expect("abort count")
                .as_f64(),
            Some(1.0)
        );
    }
}
