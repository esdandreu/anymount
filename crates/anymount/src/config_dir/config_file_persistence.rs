use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use super::read_config::open_config_file;
use async_trait::async_trait;

use crate::domain::{
    ConfigurationPersistence, CredentialUpdate, HostSaveError,
};

/// Saves credential updates into one existing native configuration record.
///
/// Known provider fields are merged into the storage section. The original
/// JSON, TOML, or YAML format and unknown fields are retained. Hosts must
/// prevent concurrent owners of the same record across processes.
#[derive(Debug, Clone)]
pub struct ConfigFilePersistence {
    path: PathBuf,
    ordering: Arc<SaveOrder>,
}

#[derive(Debug, Default)]
struct SaveOrder {
    latest_generation: AtomicU64,
    writer: Mutex<()>,
}

impl ConfigFilePersistence {
    /// Opens a uniquely named configuration record for credential saving.
    ///
    /// # Errors
    ///
    /// Returns a save error when the name is missing, ambiguous, or invalid.
    pub fn open(directory: &Path, name: &str) -> Result<Self, HostSaveError> {
        let mut matches = std::fs::read_dir(directory)
            .map_err(|_| HostSaveError)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_stem().and_then(|stem| stem.to_str()) == Some(name)
                    && matches!(
                        path.extension()
                            .and_then(|extension| extension.to_str()),
                        Some("json" | "toml" | "yaml" | "yml")
                    )
            });
        let path = matches.next().ok_or(HostSaveError)?;
        if matches.next().is_some() {
            return Err(HostSaveError);
        }
        open_config_file(&path).map_err(|_| HostSaveError)?;
        let path = path.canonicalize().map_err(|_| HostSaveError)?;
        Ok(Self {
            path,
            ordering: Arc::new(SaveOrder::default()),
        })
    }

    /// Returns the stable host identity for this configuration record.
    pub fn identity(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }

    /// Saves storage configuration without changing other document fields.
    ///
    /// Replacement is atomic within the configuration directory. New files
    /// use owner-only permissions on Unix. This cannot make provider rotation
    /// and local persistence a single crash-proof transaction.
    ///
    /// # Errors
    ///
    /// Returns a save error if reading, encoding, or replacing the file fails.
    pub fn save_storage(
        &self,
        storage: &dyn crate::domain::StorageConfig,
    ) -> Result<(), HostSaveError> {
        let mut contents = String::new();
        open_config_file(&self.path)
            .map_err(|_| HostSaveError)?
            .read_to_string(&mut contents)
            .map_err(|_| HostSaveError)?;
        let replacement = replace_storage(&self.path, &contents, storage)?;
        let directory = self.path.parent().ok_or(HostSaveError)?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)
            .map_err(|_| HostSaveError)?;
        temporary
            .write_all(replacement.as_bytes())
            .map_err(|_| HostSaveError)?;
        temporary.as_file().sync_all().map_err(|_| HostSaveError)?;
        temporary.persist(&self.path).map_err(|_| HostSaveError)?;
        Ok(())
    }
}

#[async_trait(?Send)]
impl ConfigurationPersistence for ConfigFilePersistence {
    async fn save(
        &self,
        update: &CredentialUpdate,
    ) -> Result<(), HostSaveError> {
        let generation = update.generation;
        self.ordering
            .latest_generation
            .fetch_max(generation, Ordering::AcqRel);
        let persistence = self.clone();
        let storage = update.config.clone();
        tokio::task::spawn_blocking(move || {
            let _writer = persistence
                .ordering
                .writer
                .lock()
                .map_err(|_| HostSaveError)?;
            if generation
                < persistence
                    .ordering
                    .latest_generation
                    .load(Ordering::Acquire)
            {
                return Ok(());
            }
            persistence.save_storage(storage.as_ref())
        })
        .await
        .map_err(|_| HostSaveError)?
    }
}

fn replace_storage(
    path: &Path,
    contents: &str,
    storage: &dyn crate::domain::StorageConfig,
) -> Result<String, HostSaveError> {
    let extension = path.extension().and_then(|value| value.to_str());
    let mut document: serde_json::Value = match extension {
        Some("json") => {
            serde_json::from_str(contents).map_err(|_| HostSaveError)?
        }
        Some("toml") => serde_json::to_value(
            toml::from_str::<toml::Value>(contents)
                .map_err(|_| HostSaveError)?,
        )
        .map_err(|_| HostSaveError)?,
        Some("yaml" | "yml") => serde_json::to_value(
            serde_yaml::from_str::<serde_yaml::Value>(contents)
                .map_err(|_| HostSaveError)?,
        )
        .map_err(|_| HostSaveError)?,
        _ => return Err(HostSaveError),
    };
    let replacement =
        serde_json::to_value(storage).map_err(|_| HostSaveError)?;
    let current = document
        .get_mut("storage")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or(HostSaveError)?;
    if current.get("type") != replacement.get("type")
        || replacement.get("type").and_then(serde_json::Value::as_str)
            != Some("onedrive")
    {
        return Err(HostSaveError);
    }
    for field in [
        "root",
        "endpoint",
        "client_id",
        "token_expiry_buffer_secs",
        "access_token",
        "refresh_token",
        "expires_at",
        "scopes",
        "binding",
        "credential_generation",
    ] {
        match replacement.get(field).filter(|value| !value.is_null()) {
            Some(value) => {
                current.insert(field.into(), value.clone());
            }
            None => {
                current.remove(field);
            }
        }
    }
    match extension {
        Some("json") => {
            serde_json::to_string_pretty(&document).map_err(|_| HostSaveError)
        }
        Some("toml") => toml::to_string_pretty(
            &toml::Value::try_from(document).map_err(|_| HostSaveError)?,
        )
        .map_err(|_| HostSaveError),
        Some("yaml" | "yml") => {
            serde_yaml::to_string(&document).map_err(|_| HostSaveError)
        }
        _ => Err(HostSaveError),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storages::onedrive::OneDriveConfig;
    use tempfile::TempDir;

    fn renewed_config() -> OneDriveConfig {
        serde_json::from_value(serde_json::json!({
            "access_token": "replacement-access",
            "refresh_token": "replacement-refresh",
            "root": "documents",
            "endpoint": "https://graph.microsoft.com/v1.0",
            "expires_at": 2000000000u64
        }))
        .expect("decode provider config")
    }

    #[test]
    fn rejects_ambiguous_names() {
        let directory = TempDir::new().expect("temporary directory");
        std::fs::write(directory.path().join("demo.json"), "{}")
            .expect("write json");
        std::fs::write(directory.path().join("demo.toml"), "")
            .expect("write toml");
        assert!(ConfigFilePersistence::open(directory.path(), "demo").is_err());
    }

    #[test]
    fn rejects_missing_and_path_traversal_names() {
        let directory = TempDir::new().expect("temporary directory");
        assert!(
            ConfigFilePersistence::open(directory.path(), "missing").is_err()
        );
        assert!(
            ConfigFilePersistence::open(directory.path(), "../demo").is_err()
        );
    }

    #[test]
    fn saves_json_preserving_mount_driver_and_unknown_fields() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("demo.json");
        let original = serde_json::json!({
            "path": "/mnt/demo",
            "label": "untouched",
            "driver": {"type": "host-extension", "cache": {"size": 9}},
            "storage": {"type": "onedrive", "access_token": "old",
                "future": {"keep": true}}
        });
        let mut readable = original.clone();
        readable.as_object_mut().expect("object").remove("driver");
        std::fs::write(&path, serde_json::to_vec(&readable).expect("encode"))
            .expect("write config");
        let persistence = ConfigFilePersistence::open(directory.path(), "demo")
            .expect("open record");
        std::fs::write(&path, serde_json::to_vec(&original).expect("encode"))
            .expect("add unknown driver");
        persistence
            .save_storage(&renewed_config())
            .expect("save credentials");
        let actual: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(path).expect("read replacement"),
        )
        .expect("json format retained");
        assert_eq!(actual["path"], original["path"]);
        assert_eq!(actual["driver"], original["driver"]);
        assert_eq!(actual["label"], original["label"]);
        assert_eq!(actual["storage"]["future"]["keep"], true);
        assert_eq!(actual["storage"]["refresh_token"], "replacement-refresh");
        assert_eq!(actual["storage"]["expires_at"], 2000000000u64);
    }

    #[test]
    fn saves_toml_preserving_mount_driver_and_unknown_fields() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("demo.toml");
        std::fs::write(&path, "path = '/mnt/demo'\nlabel = 'untouched'\n[storage]\ntype = 'onedrive'\naccess_token = 'old'\n[storage.future]\nkeep = true\n")
            .expect("write config");
        let persistence = ConfigFilePersistence::open(directory.path(), "demo")
            .expect("open record");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open file")
            .write_all(b"[driver]\ntype = 'host-extension'\ncache = 9\n")
            .expect("add unknown driver");
        persistence
            .save_storage(&renewed_config())
            .expect("save credentials");
        let actual: toml::Value = toml::from_str(
            &std::fs::read_to_string(path).expect("read replacement"),
        )
        .expect("toml format retained");
        assert_eq!(actual["path"].as_str(), Some("/mnt/demo"));
        assert_eq!(actual["label"].as_str(), Some("untouched"));
        assert_eq!(actual["driver"]["cache"].as_integer(), Some(9));
        assert_eq!(
            actual["storage"]["refresh_token"].as_str(),
            Some("replacement-refresh")
        );
        assert_eq!(actual["storage"]["future"]["keep"].as_bool(), Some(true));
    }

    #[test]
    fn saves_yaml_preserving_mount_driver_and_unknown_fields() {
        for extension in ["yaml", "yml"] {
            let directory = TempDir::new().expect("temporary directory");
            let path = directory.path().join(format!("demo.{extension}"));
            std::fs::write(&path, "path: /mnt/demo\nlabel: untouched\nstorage:\n  type: onedrive\n  access_token: old\n  future: {keep: true}\n")
                .expect("write config");
            let persistence =
                ConfigFilePersistence::open(directory.path(), "demo")
                    .expect("open record");
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open file")
                .write_all(b"driver:\n  type: host-extension\n  cache: 9\n")
                .expect("add unknown driver");
            persistence
                .save_storage(&renewed_config())
                .expect("save credentials");
            let actual: serde_yaml::Value = serde_yaml::from_str(
                &std::fs::read_to_string(path).expect("read replacement"),
            )
            .expect("yaml format retained");
            assert_eq!(actual["path"].as_str(), Some("/mnt/demo"));
            assert_eq!(actual["label"].as_str(), Some("untouched"));
            assert_eq!(actual["driver"]["cache"].as_i64(), Some(9));
            assert_eq!(
                actual["storage"]["refresh_token"].as_str(),
                Some("replacement-refresh")
            );
            assert_eq!(
                actual["storage"]["future"]["keep"].as_bool(),
                Some(true)
            );
        }
    }

    #[test]
    fn failed_save_does_not_replace_existing_config() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("demo.json");
        std::fs::write(
            &path,
            r#"{"path":"/mnt/demo","storage":{"type":"onedrive"}}"#,
        )
        .expect("write config");
        let persistence = ConfigFilePersistence::open(directory.path(), "demo")
            .expect("open record");
        std::fs::write(&path, "invalid sentinel-secret")
            .expect("invalidate config");
        let error = persistence
            .save_storage(&renewed_config())
            .expect_err("save fails");
        assert!(!format!("{error:?} {error}").contains("sentinel-secret"));
        assert_eq!(
            std::fs::read_to_string(path).expect("read original"),
            "invalid sentinel-secret"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_older_save_cannot_overwrite_newer_credentials() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("demo.json");
        std::fs::write(
            &path,
            r#"{"path":"/mnt/demo","storage":{"type":"onedrive"}}"#,
        )
        .expect("write config");
        let persistence = ConfigFilePersistence::open(directory.path(), "demo")
            .expect("open record");
        let ordering = persistence.ordering.clone();
        let writer = ordering.writer.lock().expect("hold file writes");
        let mut older = renewed_config();
        older.access_token = Some("older-access".to_owned());
        let older = CredentialUpdate {
            generation: 1,
            config: Box::new(older),
        };
        let newer = CredentialUpdate {
            generation: 2,
            config: Box::new(renewed_config()),
        };
        let mut old_save = Box::pin(persistence.save(&older));
        assert!(futures::poll!(&mut old_save).is_pending());
        let mut new_save = Box::pin(persistence.save(&newer));
        assert!(futures::poll!(&mut new_save).is_pending());
        drop(old_save);
        drop(writer);
        new_save.await.expect("newest save succeeds");
        let actual: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(path).expect("read replacement"),
        )
        .expect("json");
        assert_eq!(actual["storage"]["access_token"], "replacement-access");
    }
}
