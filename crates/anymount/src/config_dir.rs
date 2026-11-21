use std::path::PathBuf;

use crate::domain::{ConfigRepository, LoadedConfig};

mod config_file_persistence;
mod native_configuration;
mod read_config;

#[doc(inline)]
pub use config_file_persistence::ConfigFilePersistence;
use native_configuration::load_configuration;
use read_config::ReadConfigError;

/// Reads named configurations and preserves their format when saving credentials.
#[derive(Debug, Clone)]
pub struct ConfigDir {
    path: PathBuf,
}

impl ConfigDir {
    /// Creates a repository for an explicit configuration directory.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
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

impl ConfigRepository for ConfigDir {
    type Iter<'a> = ConfigFileIter;

    fn list(&self) -> Self::Iter<'_> {
        match std::fs::read_dir(&self.path) {
            Ok(entries) => ConfigFileIter::Entries(entries),
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    path = %self.path.display(),
                    "failed to read configuration directory",
                );
                ConfigFileIter::Empty
            }
        }
    }
}

pub enum ConfigFileIter {
    Entries(std::fs::ReadDir),
    Empty,
}

impl Iterator for ConfigFileIter {
    type Item = LoadedConfig;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Entries(entries) => entries.find_map(|entry| match entry {
                Ok(entry) => match load_configuration(&entry.path()) {
                    Ok(config) => Some(config),
                    Err(ReadConfigError::NotAConfigFile) => None,
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            path = %entry.path().display(),
                            "failed to read config",
                        );
                        None
                    }
                },
                Err(_) => None,
            }),
            Self::Empty => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ConnectStorageError;
    use tempfile::TempDir;
    use tracing_test::traced_test;

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
        match config.session.connect().await {
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
        match config.session.connect().await {
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
        match config.session.connect().await {
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
        let session = &config.session;
        let _first = session.connect().await.expect("first connection");
        let _second = session.connect().await.expect("second connection");
        let authorization = session.authorization().expect("authorization");
        assert_eq!(
            authorization.save_status(),
            crate::domain::SaveStatus::Clean
        );
        assert!(authorization.pending_update().is_none());
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
        let session = std::sync::Arc::clone(&record.session);
        assert!(matches!(
            session.connect().await,
            Err(ConnectStorageError::NotAuthenticated { .. })
        ));

        let authorization =
            record.session.authorization().expect("authorization");
        authorization.begin_device().await.expect("begin sign-in");
        authorization.poll_device().await.expect("finish sign-in");

        assert!(session.connect().await.is_ok());
        assert!(matches!(
            independent.session.connect().await,
            Err(ConnectStorageError::NotAuthenticated { .. })
        ));
        let snapshot =
            serde_json::to_value(record.config.mount.storage.as_ref())
                .expect("serialize saved config");
        assert_eq!(snapshot["password"], "");
        authorization
            .save_pending(record.persistence.as_deref().expect("persistence"))
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
        assert!(reloaded.session.connect().await.is_ok());
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
