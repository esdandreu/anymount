use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::{
    AuthorizationError, ConnectStorageError, HostSaveError, MountSession,
    SaveStatus, Storage, StorageConfig,
};
use crate::storages::onedrive::{OneDriveConfig, OneDriveSession};

use super::ConfigFilePersistence;

/// Owns shared native authorization and independent configuration persistence.
///
/// Retain this object for the lifetime of a host-owned record. Cloning it
/// shares the live credentials; loading a separate record creates a new owner.
#[derive(Clone, Debug)]
pub struct NativeConfiguration {
    session: OneDriveSession,
    persistence: ConfigFilePersistence,
}

impl NativeConfiguration {
    /// Attaches native persistence to an explicitly retained provider session.
    pub fn new(
        session: OneDriveSession,
        persistence: ConfigFilePersistence,
    ) -> Self {
        Self {
            session,
            persistence,
        }
    }

    /// Creates a native session for supported credential-bearing storage.
    ///
    /// Credential-free providers return `None` and keep their normal connection
    /// path. The caller retains the returned session instead of looking it up
    /// in a registry or comparing credential strings.
    ///
    /// # Errors
    ///
    /// Returns a setup error for invalid provider configuration.
    pub fn for_storage(
        storage: &dyn StorageConfig,
        persistence: ConfigFilePersistence,
    ) -> Result<Option<Arc<dyn MountSession>>, AuthorizationError> {
        let encoded = serde_json::to_value(storage)
            .map_err(|_| AuthorizationError::Setup)?;
        if encoded.get("type").and_then(serde_json::Value::as_str)
            != Some("onedrive")
        {
            return Ok(None);
        }
        let config: OneDriveConfig = serde_json::from_value(encoded)
            .map_err(|_| AuthorizationError::Setup)?;
        let session = OneDriveSession::new(config, persistence.identity())?;
        Ok(Some(Arc::new(Self::new(session, persistence))))
    }

    /// Returns the retained provider session for explicit host interaction.
    pub fn authorization(&self) -> &OneDriveSession {
        &self.session
    }
}

#[async_trait(?Send)]
impl MountSession for NativeConfiguration {
    fn authorization(&self) -> &dyn crate::domain::Authorization {
        &self.session
    }

    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        Ok(Box::new(self.session.connect()?))
    }

    fn save_status(&self) -> SaveStatus {
        self.session.save_status()
    }

    async fn save_pending(&self) -> Result<(), HostSaveError> {
        self.session.save_pending(&self.persistence).await
    }

    async fn wait_for_update(&self, after_generation: u64) -> u64 {
        self.session.wait_for_update(after_generation).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::StoragePath;
    use crate::test_utils::fakeoauth::FakeOAuthRuntime;
    use crate::test_utils::mocks::EmptyStorageConfig;

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

    #[test]
    fn credential_free_storage_keeps_its_connection_path() {
        let directory = tempfile::TempDir::new().expect("temporary directory");
        assert!(
            NativeConfiguration::for_storage(
                &EmptyStorageConfig,
                record(&directory)
            )
            .expect("credential-free storage")
            .is_none()
        );
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
        let owner = NativeConfiguration::new(session, persistence);
        let other = owner.clone();
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
        assert!(matches!(other.save_status(), SaveStatus::Pending { .. }));
        owner.save_pending().await.expect("save newest credentials");
        assert!(matches!(other.save_status(), SaveStatus::Saved { .. }));
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
        let owner = NativeConfiguration::new(session, persistence);
        let storage = owner.connect().await.expect("connection");
        std::fs::write(directory.path().join("drive.json"), "not json")
            .expect("break saving");
        storage
            .read_dir(StoragePath::root())
            .await
            .expect("refresh works");
        assert!(owner.save_pending().await.is_err());
        assert!(matches!(owner.save_status(), SaveStatus::Failed { .. }));
        storage
            .read_dir(StoragePath::root())
            .await
            .expect("live credentials still work");
        record(&directory);
        owner
            .save_pending()
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
