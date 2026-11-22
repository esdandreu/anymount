use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
};

use crate::domain::{
    ConfigNotFound, ConfigRecordId, ConfigRepository, CredentialUpdate,
    LoadedConfig, SaveCredentialsError,
};
use async_trait::async_trait;

mod config_file;

use config_file::{ConfigFile, ConfigFormat, ReadConfigError};

#[derive(Debug)]
struct Record {
    id: Weak<ConfigRecordId>,
    file: ConfigFile,
    committed: Mutex<Option<u64>>,
}

impl Record {
    fn save(
        &self,
        update: CredentialUpdate,
    ) -> Result<(), SaveCredentialsError> {
        let mut committed =
            self.committed.lock().map_err(|_| SaveCredentialsError)?;
        let generation = update.generation;
        if committed.is_some_and(|last| last >= generation) {
            return Ok(());
        }
        self.file.patch(update)?;
        *committed = Some(generation);
        Ok(())
    }
}

/// Reads named configurations and preserves their format when saving credentials.
///
/// Clones accept the same loaded record IDs. Saves run on Tokio's blocking pool.
#[derive(Debug, Clone)]
pub struct ConfigDir {
    path: PathBuf,
    records: Arc<Mutex<Vec<Arc<Record>>>>,
}

impl ConfigDir {
    /// Creates a repository for an explicit configuration directory.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            records: Arc::default(),
        }
    }

    fn entries(&self) -> HashMap<String, Option<PathBuf>> {
        self.records
            .lock()
            .expect("loaded records")
            .retain(|record| record.id.strong_count() != 0);
        let paths = match self.path.canonicalize().and_then(std::fs::read_dir) {
            Ok(paths) => paths,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    path = %self.path.display(),
                    "failed to read configuration directory",
                );
                return HashMap::new();
            }
        };
        let mut entries: HashMap<String, Option<PathBuf>> = HashMap::new();
        for entry in paths.filter_map(Result::ok) {
            let path = entry.path();
            if ConfigFormat::from_path(&path).is_none() {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
            else {
                continue;
            };
            entries
                .entry(name.to_owned())
                .and_modify(|entry| {
                    if entry.is_some() {
                        tracing::warn!(name, "ambiguous configuration name");
                    }
                    *entry = None;
                })
                .or_insert(Some(path));
        }
        entries
    }

    fn load(&self, path: PathBuf) -> Option<LoadedConfig> {
        let file = ConfigFile::resolve(path)?;
        let mut loaded = file
            .read()
            .and_then(|config| {
                LoadedConfig::new(
                    config,
                    file.target.to_string_lossy().into_owned(),
                )
                .map_err(|_| ReadConfigError::CannotInitializeConnector)
            })
            .inspect_err(|error| error.warn(&file.entry))
            .ok()?;
        let id = Arc::new(ConfigRecordId::default());
        self.records
            .lock()
            .expect("loaded records")
            .push(Arc::new(Record {
                id: Arc::downgrade(&id),
                file,
                committed: Mutex::new(None),
            }));
        loaded.record_id = Some(id);
        Some(loaded)
    }

    fn record(
        &self,
        id: &ConfigRecordId,
    ) -> Result<Arc<Record>, SaveCredentialsError> {
        self.records
            .lock()
            .map_err(|_| SaveCredentialsError)?
            .iter()
            .find(|record| std::ptr::eq(record.id.as_ptr(), id))
            .cloned()
            .ok_or(SaveCredentialsError)
    }
}

/// Returns the default configuration directory for the application.
fn default_config_dir() -> PathBuf {
    match dirs::config_local_dir() {
        Some(config_local_dir) => config_local_dir.join("anymount"),
        None => match std::env::current_dir() {
            Ok(current_dir) => current_dir,
            Err(_) => PathBuf::from("."),
        },
    }
}

impl Default for ConfigDir {
    fn default() -> Self {
        ConfigDir::new(default_config_dir())
    }
}

#[async_trait(?Send)]
impl ConfigRepository for ConfigDir {
    fn list(&self) -> Box<dyn Iterator<Item = LoadedConfig> + '_> {
        Box::new(
            self.entries()
                .into_values()
                .flatten()
                .filter_map(|path| self.load(path)),
        )
    }

    fn find(&self, name: &str) -> Result<LoadedConfig, ConfigNotFound> {
        self.entries()
            .remove(name)
            .flatten()
            .and_then(|path| self.load(path))
            .ok_or_else(|| ConfigNotFound {
                name: name.to_owned(),
            })
    }

    async fn save_credentials(
        &self,
        record_id: &ConfigRecordId,
        update: CredentialUpdate,
    ) -> Result<(), SaveCredentialsError> {
        let record = self.record(record_id)?;
        tokio::task::spawn_blocking(move || record.save(update))
            .await
            .map_err(|_| SaveCredentialsError)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ConnectStorageError, StorageConnector, StoragePath};
    use crate::storages::onedrive::{OneDriveConfig, OneDriveConnector};
    use crate::test_utils::fakeoauth::FakeOAuthRuntime;
    use tempfile::TempDir;
    use tracing_test::traced_test;

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

    fn write_record(directory: &tempfile::TempDir) {
        std::fs::write(directory.path().join("drive.json"),
            r#"{"path":"/mnt/drive","label":"keep","storage":{"type":"onedrive"}}"#)
            .expect("write configuration");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn named_records_do_not_require_provider_authorization() {
        let directory = tempfile::TempDir::new().expect("temporary directory");
        std::fs::write(
            directory.path().join("local.json"),
            r#"{"path":"/mnt/local","storage":{"type":"empty"}}"#,
        )
        .expect("write local configuration");
        let repository = ConfigDir::new(directory.path().to_path_buf());
        let record =
            repository.find("local").expect("load local configuration");
        assert!(record.record_id.is_some());
        assert!(record.connector.authorization().is_none());
        assert!(record.connector.connect().await.is_ok());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn connections_share_refresh_but_not_independent_record_owners() {
        let directory = tempfile::TempDir::new().expect("temporary directory");
        write_record(&directory);
        let repository = ConfigDir::new(directory.path().to_path_buf());
        let record = repository.find("drive").expect("load record");
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
        let connector = OneDriveConnector::with_runtime(
            provider_config(),
            directory
                .path()
                .join("drive.json")
                .canonicalize()
                .expect("path")
                .to_string_lossy()
                .into_owned(),
            runtime.runtime(),
        )
        .expect("session");
        let independent = OneDriveConnector::with_runtime(
            provider_config(),
            "independent-copy".to_owned(),
            runtime.runtime(),
        )
        .expect("independent session");
        let owner: Arc<dyn StorageConnector> = Arc::new(connector);
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
        let update = authorization
            .credential_snapshot(0)
            .expect("refresh update");
        let generation = update.generation;
        repository
            .save_credentials(
                record.record_id.as_deref().expect("record ID"),
                update,
            )
            .await
            .expect("save newest credentials");
        assert!(authorization.credential_snapshot(generation).is_none());
        assert!(independent.credential_snapshot(0).is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn save_failure_retains_live_credentials_and_retry_needs_no_exchange()
    {
        let directory = tempfile::TempDir::new().expect("temporary directory");
        write_record(&directory);
        let repository = ConfigDir::new(directory.path().to_path_buf());
        let record = repository.find("drive").expect("load record");
        let record_id = record.record_id.as_deref().expect("record ID");
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
        let connector = OneDriveConnector::with_runtime(
            provider_config(),
            directory
                .path()
                .join("drive.json")
                .canonicalize()
                .expect("path")
                .to_string_lossy()
                .into_owned(),
            runtime.runtime(),
        )
        .expect("session");
        let owner: Arc<dyn StorageConnector> = Arc::new(connector);
        let authorization = owner.authorization().expect("authorization");
        let storage = owner.connect().await.expect("connection");
        std::fs::write(directory.path().join("drive.json"), "not json")
            .expect("break saving");
        storage
            .read_dir(StoragePath::root())
            .await
            .expect("refresh works");
        let snapshot = authorization
            .credential_snapshot(0)
            .expect("refresh update");
        let generation = snapshot.generation;
        assert!(
            repository
                .save_credentials(record_id, snapshot)
                .await
                .is_err()
        );
        storage
            .read_dir(StoragePath::root())
            .await
            .expect("live credentials still work");
        write_record(&directory);
        let snapshot = authorization
            .credential_snapshot(0)
            .expect("retained update");
        assert_eq!(snapshot.generation, generation);
        repository
            .save_credentials(record_id, snapshot)
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

    #[tokio::test(flavor = "current_thread")]
    async fn reads_toml_config_file() {
        let dir = TempDir::new().expect("create temp dir");
        std::fs::write(
            &dir.path().join("demo.toml"),
            r#"
path = "/mnt/test"
[storage]
type = "cannot-connect"
error_message = "toml test"
"#,
        )
        .expect("write toml config");
        let config_dir = ConfigDir::new(dir.path().to_path_buf());
        let mounts = config_dir.list().collect::<Vec<_>>();
        let config = mounts.into_iter().next().expect("read config");
        assert_eq!(config.config.mount.name, "demo");
        match config.connector.connect().await {
            Ok(_) => panic!("should not connect"),
            Err(ConnectStorageError::Failed { message, .. }) => {
                assert_eq!(message, "toml test");
            }
            Err(error) => panic!("unexpected connection error: {error}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reads_json_config_file() {
        let dir = TempDir::new().expect("create temp dir");
        std::fs::write(
            &dir.path().join("demo.json"),
            r#"{
    "path": "/mnt/test",
    "storage": {
        "type": "cannot-connect",
        "error_message": "json test"
    }
}"#,
        )
        .expect("write json config");

        let config_dir = ConfigDir::new(dir.path().to_path_buf());
        let mounts = config_dir.list().collect::<Vec<_>>();
        let config = mounts.into_iter().next().expect("read config");
        assert_eq!(config.config.mount.name, "demo");
        match config.connector.connect().await {
            Ok(_) => panic!("should not connect"),
            Err(ConnectStorageError::Failed { message, .. }) => {
                assert_eq!(message, "json test");
            }
            Err(error) => panic!("unexpected connection error: {error}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reads_yaml_config_file() {
        let dir = TempDir::new().expect("create temp dir");
        std::fs::write(
            &dir.path().join("demo.yaml"),
            r#"
path: /mnt/test
storage:
  type: cannot-connect
  error_message: yaml test
"#,
        )
        .expect("write yaml config");

        let config_dir = ConfigDir::new(dir.path().to_path_buf());
        let mounts = config_dir.list().collect::<Vec<_>>();
        let config = mounts.into_iter().next().expect("read config");
        assert_eq!(config.config.mount.name, "demo");
        match config.connector.connect().await {
            Ok(_) => panic!("should not connect"),
            Err(ConnectStorageError::Failed { message, .. }) => {
                assert_eq!(message, "yaml test");
            }
            Err(error) => panic!("unexpected connection error: {error}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retains_a_host_session_for_a_named_onedrive_record() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("drive.json");
        let original = r#"{
            "path": "/mnt/drive",
            "storage": {
                "type": "onedrive",
                "root": "/documents",
                "endpoint": "https://graph.microsoft.com/v1.0",
                "access_token": "opaque-access-only"
            }
        }"#;
        std::fs::write(&path, original).expect("write named record");
        let repository = ConfigDir::new(directory.path().to_path_buf());
        let config = repository.find("drive").expect("load named record");
        let connector = &config.connector;
        let _first = connector.connect().await.expect("first connection");
        let _second = connector.connect().await.expect("second connection");
        let authorization = connector.authorization().expect("authorization");
        assert!(authorization.credential_snapshot(0).is_none());
        assert_eq!(
            std::fs::read_to_string(path).expect("read original"),
            original
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn loads_and_saves_a_non_onedrive_provider_without_host_dispatch() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("fixture.json");
        let original = serde_json::json!({
            "path": "/mnt/fixture",
            "storage": {
                "type": "test", "bucket": "files", "username": "user",
                "password": "", "retired_code": "obsolete",
                "extension": {"arbitrary": [1, true]}
            }
        });
        std::fs::write(&path, original.to_string()).expect("write fixture");
        let repository = ConfigDir::new(directory.path().to_path_buf());
        let record = repository.find("fixture").expect("load fixture");
        let independent = repository.find("fixture").expect("reload fixture");
        let connector = std::sync::Arc::clone(&record.connector);
        assert!(matches!(
            connector.connect().await,
            Err(ConnectStorageError::NotAuthenticated { .. })
        ));

        let authorization =
            record.connector.authorization().expect("authorization");
        authorization.begin_device().await.expect("begin sign-in");
        authorization.poll_device().await.expect("finish sign-in");

        assert!(connector.connect().await.is_ok());
        assert!(matches!(
            independent.connector.connect().await,
            Err(ConnectStorageError::NotAuthenticated { .. })
        ));
        let snapshot =
            serde_json::to_value(record.config.mount.storage.as_ref())
                .expect("serialize saved config");
        assert_eq!(snapshot["password"], "");
        let update = authorization
            .credential_snapshot(0)
            .expect("sign-in update");
        repository
            .save_credentials(
                record.record_id.as_deref().expect("record ID"),
                update,
            )
            .await
            .expect("save rotated fixture credentials");
        let saved: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(path).expect("saved fixture"),
        )
        .expect("JSON file");
        assert_eq!(saved["storage"]["type"], "test");
        assert_eq!(saved["storage"]["password"], "rotated-password");
        assert_eq!(
            saved["storage"]["extension"],
            original["storage"]["extension"]
        );
        assert!(saved["storage"].get("retired_code").is_none());
        let reloaded = repository.find("fixture").expect("load saved fixture");
        assert!(reloaded.connector.connect().await.is_ok());
    }

    #[test]
    #[traced_test]
    fn warns_config_errors() {
        let dir = TempDir::new().expect("create temp dir");
        std::fs::write(dir.path().join("demo.toml"), r#"not toml"#)
            .expect("write toml config");
        let config = ConfigDir::new(dir.path().to_path_buf());
        let mounts = config.list().collect::<Vec<_>>();
        assert!(mounts.is_empty());
        assert!(logs_contain("failed to read config"));
    }
}
