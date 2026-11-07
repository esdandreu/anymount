#![cfg(target_arch = "wasm32")]

use std::path::PathBuf;

use anymount::domain::{Config, MountConfig, StorageConfig};
use anymount::drivers::{
    browser::BrowserDriverConfig, get_default_driver_config,
};
use js_sys::{Function, Reflect};
use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

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
        anymount::read_dir(storage.as_ref(), PathBuf::from("../outside")),
        Err(anymount::ReadDirError::InvalidPath(_))
    ));
    let _read_file_at = anymount::read_file_at::<dyn anymount::Storage>;
}

#[wasm_bindgen_test]
async fn shared_mount_uses_registered_browser_driver_and_storage_config() {
    let storage: Box<dyn StorageConfig> =
        serde_json::from_str(r#"{"type":"browser-placeholder"}"#)
            .expect("browser storage configuration is registered");
    let global = js_sys::global();
    let picker = Function::new_no_args("return Promise.resolve({})");
    Reflect::set(&global, &JsValue::from_str("showDirectoryPicker"), &picker)
        .expect("install directory picker");

    let config = Config {
        driver: None,
        mount: MountConfig {
            name: "workspace".to_owned(),
            path: PathBuf::from("workspace"),
            storage,
        },
    };
    let mount = anymount::mount(config).await.expect("mount workspace");

    assert_eq!(mount.name(), "workspace");
    assert!(!mount.is_connected());
    Reflect::delete_property(
        &global,
        &JsValue::from_str("showDirectoryPicker"),
    )
    .expect("remove directory picker");
}
