mod browser_driver;

pub use browser_driver::{
    BrowserDriver, BrowserDriverApi, BrowserDriverConfig,
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
                getFileHandle: async (name, options) => {
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
        assert_eq!(storage.read(path).await.expect("read"), vec![0, 255, 42]);
        Reflect::delete_property(
            &global,
            &JsValue::from_str("showDirectoryPicker"),
        )
        .expect("remove picker");
    }
}
