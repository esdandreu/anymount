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
            serde_json::from_str(r#"{"type":"browser-placeholder"}"#)
                .expect("browser storage configuration is registered");
        let storage = config.connect().expect("connect storage");

        assert!(matches!(
            crate::read_dir(storage.as_ref(), PathBuf::from("../outside")),
            Err(crate::ReadDirError::InvalidPath(_))
        ));
        let _read_file_at = crate::read_file_at::<dyn crate::Storage>;
    }

    #[wasm_bindgen_test]
    async fn shared_mount_uses_registered_browser_driver_and_storage_config() {
        let storage: Box<dyn StorageConfig> =
            serde_json::from_str(r#"{"type":"browser-placeholder"}"#)
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
}
