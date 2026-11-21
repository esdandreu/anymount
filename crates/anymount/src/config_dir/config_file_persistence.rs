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
/// Only provider-designated paths are changed.
/// The original JSON, TOML, or YAML format and unrelated fields are retained.
/// Hosts must prevent concurrent owners of a record across processes.
#[derive(Debug, Clone)]
pub struct ConfigFilePersistence(Arc<Record>);

#[derive(Debug)]
struct Record {
    path: PathBuf,
    format: &'static str,
    ordering: SaveOrder,
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
        let format = match path.extension().and_then(|value| value.to_str()) {
            Some("json") => "json",
            Some("toml") => "toml",
            Some("yaml" | "yml") => "yaml",
            _ => return Err(HostSaveError),
        };
        let path = path.canonicalize().map_err(|_| HostSaveError)?;
        Ok(Self(Arc::new(Record {
            path,
            format,
            ordering: SaveOrder::default(),
        })))
    }

    /// Returns the stable host identity for this configuration record.
    pub fn identity(&self) -> String {
        self.0.path.to_string_lossy().into_owned()
    }

    fn write_storage(
        &self,
        fields: StorageFields,
    ) -> Result<(), HostSaveError> {
        let mut contents = String::new();
        open_config_file(&self.0.path)
            .map_err(|_| HostSaveError)?
            .read_to_string(&mut contents)
            .map_err(|_| HostSaveError)?;
        let replacement = replace_storage(self.0.format, &contents, fields)?;
        let directory = self.0.path.parent().ok_or(HostSaveError)?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)
            .map_err(|_| HostSaveError)?;
        temporary
            .write_all(replacement.as_bytes())
            .map_err(|_| HostSaveError)?;
        temporary.as_file().sync_all().map_err(|_| HostSaveError)?;
        temporary.persist(&self.0.path).map_err(|_| HostSaveError)?;
        Ok(())
    }
}

#[async_trait(?Send)]
impl ConfigurationPersistence for ConfigFilePersistence {
    async fn save(
        &self,
        update: &CredentialUpdate,
    ) -> Result<(), HostSaveError> {
        let fields = StorageFields {
            provider: update.storage_type,
            snapshot: serde_json::to_value(&update.config)
                .map_err(|_| HostSaveError)?,
            paths: update.fields,
        };
        let generation = update.generation;
        self.0
            .ordering
            .latest_generation
            .fetch_max(generation, Ordering::AcqRel);
        let persistence = self.clone();
        tokio::task::spawn_blocking(move || {
            let ordering = &persistence.0.ordering;
            let _writer = ordering.writer.lock().map_err(|_| HostSaveError)?;
            if generation < ordering.latest_generation.load(Ordering::Acquire) {
                return Ok(());
            }
            persistence.write_storage(fields)
        })
        .await
        .map_err(|_| HostSaveError)?
    }
}

struct StorageFields {
    provider: &'static str,
    snapshot: serde_json::Value,
    paths: &'static [&'static [&'static str]],
}

fn replace_storage(
    format: &str,
    contents: &str,
    fields: StorageFields,
) -> Result<String, HostSaveError> {
    match format {
        "json" => {
            let mut document: serde_json::Value =
                serde_json::from_str(contents).map_err(|_| HostSaveError)?;
            StorageNode::Json(
                document.get_mut("storage").ok_or(HostSaveError)?,
            )
            .apply(&fields)?;
            serde_json::to_string_pretty(&document).map_err(|_| HostSaveError)
        }
        "toml" => {
            let mut document: toml::Value =
                toml::from_str(contents).map_err(|_| HostSaveError)?;
            StorageNode::Toml(
                document.get_mut("storage").ok_or(HostSaveError)?,
            )
            .apply(&fields)?;
            toml::to_string_pretty(&document).map_err(|_| HostSaveError)
        }
        "yaml" => {
            let mut document: serde_yaml::Value =
                serde_yaml::from_str(contents).map_err(|_| HostSaveError)?;
            StorageNode::Yaml(
                document.get_mut("storage").ok_or(HostSaveError)?,
            )
            .apply(&fields)?;
            serde_yaml::to_string(&document).map_err(|_| HostSaveError)
        }
        _ => Err(HostSaveError),
    }
}

enum StorageNode<'a> {
    Json(&'a mut serde_json::Value),
    Toml(&'a mut toml::Value),
    Yaml(&'a mut serde_yaml::Value),
}

impl StorageNode<'_> {
    fn apply(&mut self, fields: &StorageFields) -> Result<(), HostSaveError> {
        let provider = match self {
            Self::Json(node) => {
                node.get("type").and_then(serde_json::Value::as_str)
            }
            Self::Toml(node) => node.get("type").and_then(toml::Value::as_str),
            Self::Yaml(node) => {
                node.get("type").and_then(serde_yaml::Value::as_str)
            }
        };
        if provider != Some(fields.provider) {
            return Err(HostSaveError);
        }
        for path in fields.paths {
            if path.is_empty() || path[0] == "type" {
                return Err(HostSaveError);
            }
            let value = path
                .iter()
                .try_fold(&fields.snapshot, |node, key| node.get(*key))
                .filter(|value| !value.is_null());
            self.edit(path, value)?;
        }
        Ok(())
    }

    fn edit(
        &mut self,
        path: &[&str],
        value: Option<&serde_json::Value>,
    ) -> Result<(), HostSaveError> {
        let (&key, tail) = path.split_first().ok_or(HostSaveError)?;
        if !tail.is_empty() {
            if let Some(mut child) = self.child(key, value.is_some())? {
                child.edit(tail, value)?;
            }
            return Ok(());
        }
        match self {
            Self::Json(node) => {
                let map = node.as_object_mut().ok_or(HostSaveError)?;
                if let Some(value) = value {
                    map.insert(key.into(), value.clone());
                } else {
                    map.remove(key);
                }
            }
            Self::Toml(node) => {
                let map = node.as_table_mut().ok_or(HostSaveError)?;
                if let Some(value) = value {
                    map.insert(
                        key.into(),
                        toml::Value::try_from(value)
                            .map_err(|_| HostSaveError)?,
                    );
                } else {
                    map.remove(key);
                }
            }
            Self::Yaml(node) => {
                let map = node.as_mapping_mut().ok_or(HostSaveError)?;
                if let Some(value) = value {
                    map.insert(
                        key.into(),
                        serde_yaml::to_value(value)
                            .map_err(|_| HostSaveError)?,
                    );
                } else {
                    map.remove(key);
                }
            }
        }
        Ok(())
    }

    fn child(
        &mut self,
        key: &str,
        create: bool,
    ) -> Result<Option<StorageNode<'_>>, HostSaveError> {
        match self {
            Self::Json(node) => {
                let map = node.as_object_mut().ok_or(HostSaveError)?;
                if create {
                    let child =
                        map.entry(key).or_insert(serde_json::Value::Null);
                    if child.is_null() {
                        *child = serde_json::Value::Object(Default::default());
                    }
                    Ok(Some(StorageNode::Json(child)))
                } else {
                    Ok(map
                        .get_mut(key)
                        .filter(|child| !child.is_null())
                        .map(StorageNode::Json))
                }
            }
            Self::Toml(node) => {
                let map = node.as_table_mut().ok_or(HostSaveError)?;
                let child = if create {
                    Some(map.entry(key).or_insert_with(|| {
                        toml::Value::Table(Default::default())
                    }))
                } else {
                    map.get_mut(key)
                };
                Ok(child.map(StorageNode::Toml))
            }
            Self::Yaml(node) => {
                let map = node.as_mapping_mut().ok_or(HostSaveError)?;
                if create {
                    let child = map
                        .entry(key.into())
                        .or_insert(serde_yaml::Value::Null);
                    if child.is_null() {
                        *child = serde_yaml::Value::Mapping(Default::default());
                    }
                    Ok(Some(StorageNode::Yaml(child)))
                } else {
                    Ok(map
                        .get_mut(key)
                        .filter(|child| !child.is_null())
                        .map(StorageNode::Yaml))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storages::onedrive::OneDriveConfig;
    use tempfile::TempDir;

    fn renewed_config(generation: u64, access_token: &str) -> CredentialUpdate {
        let config: OneDriveConfig = serde_json::from_value(serde_json::json!({
            "access_token": access_token,
            "refresh_token": "replacement-refresh",
            "root": "documents",
            "endpoint": "https://graph.microsoft.com/v1.0",
            "expires_at": 2000000000u64,
            "binding": {"provider":"onedrive", "client_id":"new", "authority":"issuer", "resource":"resource"}
        }))
        .expect("decode provider config");
        CredentialUpdate {
            generation,
            config: Box::new(config),
            storage_type: "onedrive",
            fields: &[
                &["access_token"],
                &["refresh_token"],
                &["expires_at"],
                &["client_id"],
                &["binding", "client_id"],
            ],
        }
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

    #[tokio::test(flavor = "current_thread")]
    async fn saves_json_preserving_mount_driver_and_unknown_fields() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("demo.json");
        let original = serde_json::json!({
            "path": "/mnt/demo",
            "label": "untouched",
            "driver": {"type": "host-extension", "cache": {"size": 9}},
            "storage": {"type": "onedrive", "access_token": "old",
                "root": "keep-root", "client_id": "obsolete",
                "binding": {"client_id": "old", "future": "keep-binding-field"},
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
            .save(&renewed_config(1, "replacement-access"))
            .await
            .expect("save credentials");
        let actual: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(path).expect("read replacement"),
        )
        .expect("json format retained");
        assert_eq!(actual["path"], original["path"]);
        assert_eq!(actual["driver"], original["driver"]);
        assert_eq!(actual["label"], original["label"]);
        assert_eq!(actual["storage"]["future"]["keep"], true);
        assert_eq!(actual["storage"]["root"], "keep-root");
        assert_eq!(actual["storage"]["binding"]["client_id"], "new");
        assert_eq!(
            actual["storage"]["binding"]["future"],
            "keep-binding-field"
        );
        assert!(actual["storage"].get("client_id").is_none());
        assert_eq!(actual["storage"]["refresh_token"], "replacement-refresh");
        assert_eq!(actual["storage"]["expires_at"], 2000000000u64);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn saves_toml_preserving_mount_driver_and_unknown_fields() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("demo.toml");
        std::fs::write(&path, "path = '/mnt/demo'\nlabel = 'untouched'\nwritten_at = 2025-01-20T06:00:00Z\n[storage]\ntype = 'onedrive'\naccess_token = 'old'\n[storage.future]\nkeep = true\n")
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
            .save(&renewed_config(1, "replacement-access"))
            .await
            .expect("save credentials");
        let actual: toml::Value = toml::from_str(
            &std::fs::read_to_string(path).expect("read replacement"),
        )
        .expect("toml format retained");
        assert_eq!(actual["path"].as_str(), Some("/mnt/demo"));
        assert_eq!(actual["label"].as_str(), Some("untouched"));
        assert!(actual["written_at"].as_datetime().is_some());
        assert_eq!(actual["driver"]["cache"].as_integer(), Some(9));
        assert_eq!(
            actual["storage"]["refresh_token"].as_str(),
            Some("replacement-refresh")
        );
        assert_eq!(actual["storage"]["future"]["keep"].as_bool(), Some(true));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn saves_yaml_preserving_mount_driver_and_unknown_fields() {
        for extension in ["yaml", "yml"] {
            let directory = TempDir::new().expect("temporary directory");
            let path = directory.path().join(format!("demo.{extension}"));
            std::fs::write(&path, "path: /mnt/demo\nlabel: untouched\nstorage:\n  type: onedrive\n  access_token: old\n  future: {keep: true, 7: 'numeric key'}\n")
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
                .save(&renewed_config(1, "replacement-access"))
                .await
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
            assert_eq!(
                actual["storage"]["future"]
                    .as_mapping()
                    .expect("preserve YAML mapping")
                    .get(&serde_yaml::Value::from(7)),
                Some(&serde_yaml::Value::from("numeric key"))
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failed_save_does_not_replace_existing_config() {
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
            .save(&renewed_config(1, "replacement-access"))
            .await
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
        let writer = persistence
            .0
            .ordering
            .writer
            .lock()
            .expect("hold file writes");
        let older = renewed_config(1, "older-access");
        let newer = renewed_config(2, "replacement-access");
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
