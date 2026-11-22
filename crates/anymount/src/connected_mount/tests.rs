use super::*;
use crate::ConfigDir;
use crate::domain::{SaveCredentialsError, Storage, StoragePath};
use crate::storages::onedrive::OneDriveConnector;
use crate::test_utils::{
    fakeoauth::{FakeOAuthRuntime, ObservedConfigRepository},
    mocks::NoOpDriverConfig,
};
use futures::StreamExt;
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};
use tempfile::tempdir;

const TIMEOUT: Duration = Duration::from_secs(5);
const ORIGINAL: &str = r#"{
    "path": "/mnt/drive", "label": "untouched",
    "storage": {"type": "onedrive", "access_token": "old-access",
                "refresh_token": "old-refresh", "expires_at": 1}
}"#;

fn load(path: &Path) -> (LoadedConfig, ConfigDir, FakeOAuthRuntime) {
    fs::write(path, ORIGINAL).expect("write configuration");
    let repository = ConfigDir::new(path.parent().expect("directory").into());
    let mut record = repository.find("drive").expect("record");
    let config: Value = serde_json::from_str(ORIGINAL).expect("json");
    let protocol = FakeOAuthRuntime::new(1000);
    record.connector = Arc::new(
        OneDriveConnector::with_runtime(
            serde_json::from_value(config["storage"].clone())
                .expect("provider"),
            path.canonicalize()
                .expect("path")
                .to_string_lossy()
                .into_owned(),
            protocol.runtime(),
        )
        .expect("session"),
    );
    record.config.driver = Some(Box::new(NoOpDriverConfig));
    (record, repository, protocol)
}

fn refresh(protocol: &FakeOAuthRuntime, token: &str) {
    protocol.push_json(
        200,
        json!({
            "access_token": token, "refresh_token": token,
            "token_type": "Bearer", "expires_in": 3600
        }),
    );
    protocol.push_json(200, json!({"value": []}));
}

async fn read_root(storage: &dyn Storage) {
    assert_eq!(
        storage
            .read_dir(StoragePath::root())
            .await
            .expect("list root")
            .count()
            .await,
        0
    );
}

#[tokio::test]
async fn saves_catch_up_without_blocking_reads() {
    let directory = tempdir().expect("directory");
    let path = directory.path().join("drive.json");
    let (record, repository, protocol) = load(&path);
    let (repository, outcomes) = ObservedConfigRepository::new(repository);
    let (started, release) = repository.pause_next_save();
    refresh(&protocol, "first");
    protocol.push_json(
        401,
        json!({"error": {"code": "InvalidAuthenticationToken"}}),
    );
    refresh(&protocol, "latest");

    let host = ConnectedMount::connect(&record, Arc::new(repository))
        .await
        .expect("host connection");
    let release = release;
    let storage = record.connector.connect().await.expect("storage");
    read_root(storage.as_ref()).await;
    assert_eq!(
        started.recv_timeout(TIMEOUT).expect("first save started"),
        1
    );
    read_root(storage.as_ref()).await;
    assert!(outcomes.try_recv().is_err());
    release.send(()).expect("release save");
    assert_eq!(
        outcomes.recv_timeout(TIMEOUT).expect("first save"),
        (1, Ok(()))
    );
    assert_eq!(
        outcomes.recv_timeout(TIMEOUT).expect("latest save"),
        (2, Ok(()))
    );
    let saved: Value =
        serde_json::from_slice(&fs::read(path).expect("file")).expect("json");
    assert_eq!(saved["storage"]["access_token"], "latest");
    assert_eq!(saved["label"], "untouched");
    host.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn failed_save_leaves_storage_usable_and_retries_before_host_exit() {
    for exit in ["disconnect", "unregister", "drop"] {
        let directory = tempdir().expect("directory");
        let path = directory.path().join("drive.json");
        let (record, repository, protocol) = load(&path);
        let (repository, outcomes) = ObservedConfigRepository::new(repository);
        refresh(&protocol, "renewed");
        protocol.push_json(200, json!({"value": []}));
        fs::write(&path, "invalid json").expect("make persistence fail");

        let host = ConnectedMount::connect(&record, Arc::new(repository))
            .await
            .expect("host connection");
        let storage = record.connector.connect().await.expect("storage");
        read_root(storage.as_ref()).await;
        assert_eq!(
            outcomes.recv_timeout(TIMEOUT).expect("background save"),
            (1, Err(SaveCredentialsError))
        );
        read_root(storage.as_ref()).await;
        fs::write(&path, ORIGINAL).expect("repair persistence");
        match exit {
            "disconnect" => host.disconnect().await.expect("disconnect"),
            "unregister" => host.unregister().await.expect("unregister"),
            _ => drop(host),
        }
        assert_eq!(
            outcomes.try_recv().expect("final save finished"),
            (1, Ok(()))
        );
        let saved: Value =
            serde_json::from_slice(&fs::read(path).expect("file"))
                .expect("json");
        assert_eq!(saved["storage"]["refresh_token"], "renewed");
        assert_eq!(protocol.requests().len(), 3);
    }
}
