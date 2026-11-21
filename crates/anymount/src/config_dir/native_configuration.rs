use std::{path::Path, sync::Arc};

use crate::domain::LoadedConfig;

use super::{
    ConfigFilePersistence,
    read_config::{ReadConfigError, read_config},
};

pub fn load_configuration(
    path: &Path,
) -> Result<LoadedConfig, ReadConfigError> {
    let config = read_config(path)?;
    let persistence = ConfigFilePersistence::open(
        path.parent().ok_or(ReadConfigError::NotAConfigFile)?,
        &config.mount.name,
    )
    .map_err(|_| ReadConfigError::CannotInitializeSession)?;
    let mut record = LoadedConfig::new(config, persistence.identity())
        .map_err(|_| ReadConfigError::CannotInitializeSession)?;
    if record.session.authorization().is_some() {
        record.persistence = Some(Arc::new(persistence));
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{SaveStatus, StoragePath, StorageSession};
    use crate::storages::onedrive::{OneDriveConfig, OneDriveSession};
    use crate::test_utils::fakeoauth::FakeOAuthRuntime;

    fn provider_config() -> OneDriveConfig {
        serde_json::from_value(serde_json::json!({
            "root": "/",
            "endpoint": "https://graph.microsoft.com/v1.0",
            "access_token": "old-access",
            "refresh_token": "old-refresh",
            "expires_at": 1
        }))
        .expect("provider configuration")
    }

    fn record(directory: &tempfile::TempDir) -> ConfigFilePersistence {
        std::fs::write(directory.path().join("drive.json"),
            r#"{"path":"/mnt/drive","label":"keep","storage":{"type":"onedrive"}}"#)
            .expect("write configuration");
        ConfigFilePersistence::open(directory.path(), "drive")
            .expect("open record")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn credential_free_storage_keeps_its_connection_path() {
        let directory = tempfile::TempDir::new().expect("temporary directory");
        std::fs::write(
            directory.path().join("local.json"),
            r#"{"path":"/mnt/local","storage":{"type":"empty"}}"#,
        )
        .expect("write local configuration");
        let record = load_configuration(&directory.path().join("local.json"))
            .expect("load local configuration");
        assert!(record.session.authorization().is_none());
        assert!(record.persistence.is_none());
        assert!(record.session.connect().await.is_ok());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn connections_share_refresh_but_not_independent_record_owners() {
        let directory = tempfile::TempDir::new().expect("temporary directory");
        let persistence = record(&directory);
        let runtime = FakeOAuthRuntime::new(1000);
        runtime.push_json(
            200,
            serde_json::json!({
                "access_token": "new-access", "refresh_token": "new-refresh",
                "token_type": "Bearer", "expires_in": 3600
            }),
        );
        runtime.push_json(200, serde_json::json!({"value": []}));
        runtime.push_json(200, serde_json::json!({"value": []}));
        let session = OneDriveSession::with_runtime(
            provider_config(),
            persistence.identity(),
            runtime.runtime(),
        )
        .expect("session");
        let independent = OneDriveSession::with_runtime(
            provider_config(),
            "independent-copy".to_owned(),
            runtime.runtime(),
        )
        .expect("independent session");
        let owner: Arc<dyn StorageSession> = Arc::new(session);
        let other = Arc::clone(&owner);
        let first = owner.connect().await.expect("first connection");
        let second = other.connect().await.expect("second connection");
        let (first, second) = tokio::join!(
            first.read_dir(StoragePath::root()),
            second.read_dir(StoragePath::root()),
        );
        assert!(first.is_ok());
        assert!(second.is_ok());
        assert_eq!(
            runtime
                .requests()
                .iter()
                .filter(|request| request.method == "POST")
                .count(),
            1
        );
        let authorization = other.authorization().expect("authorization");
        assert!(matches!(
            authorization.save_status(),
            SaveStatus::Pending { .. }
        ));
        owner
            .authorization()
            .expect("authorization")
            .save_pending(&persistence)
            .await
            .expect("save newest credentials");
        assert!(matches!(
            authorization.save_status(),
            SaveStatus::Saved { .. }
        ));
        assert_eq!(independent.save_status(), SaveStatus::Clean);
        assert!(independent.pending_update().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn save_failure_retains_live_credentials_and_retry_needs_no_exchange()
    {
        let directory = tempfile::TempDir::new().expect("temporary directory");
        let persistence = record(&directory);
        let runtime = FakeOAuthRuntime::new(1000);
        runtime.push_json(
            200,
            serde_json::json!({
                "access_token": "new-access", "refresh_token": "new-refresh",
                "token_type": "Bearer", "expires_in": 3600
            }),
        );
        runtime.push_json(200, serde_json::json!({"value": []}));
        runtime.push_json(200, serde_json::json!({"value": []}));
        let session = OneDriveSession::with_runtime(
            provider_config(),
            persistence.identity(),
            runtime.runtime(),
        )
        .expect("session");
        let owner: Arc<dyn StorageSession> = Arc::new(session);
        let authorization = owner.authorization().expect("authorization");
        let storage = owner.connect().await.expect("connection");
        std::fs::write(directory.path().join("drive.json"), "not json")
            .expect("break saving");
        storage
            .read_dir(StoragePath::root())
            .await
            .expect("refresh works");
        assert!(authorization.save_pending(&persistence).await.is_err());
        assert!(matches!(
            authorization.save_status(),
            SaveStatus::Failed { .. }
        ));
        storage
            .read_dir(StoragePath::root())
            .await
            .expect("live credentials still work");
        record(&directory);
        authorization
            .save_pending(&persistence)
            .await
            .expect("retry saves existing snapshot");
        let saved: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(directory.path().join("drive.json"))
                .expect("read config"),
        )
        .expect("json");
        assert_eq!(saved["storage"]["refresh_token"], "new-refresh");
        assert_eq!(saved["label"], "keep");
        assert_eq!(runtime.requests().len(), 3);
    }
}
