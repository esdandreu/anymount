use std::io::Write;

use super::{ConfigFile, ConfigFormat};
use crate::domain::{CredentialUpdate, SaveCredentialsError};

impl ConfigFile {
    pub fn patch(
        &self,
        update: CredentialUpdate,
    ) -> Result<(), SaveCredentialsError> {
        let fields = StorageFields {
            provider: update.storage_type,
            snapshot: serde_json::to_value(&update.config)
                .map_err(|_| SaveCredentialsError)?,
            paths: update.fields,
        };
        self.write_storage(fields)
    }

    fn write_storage(
        &self,
        fields: StorageFields,
    ) -> Result<(), SaveCredentialsError> {
        // A replaced alias must be loaded again before saving to it.
        if self
            .entry
            .canonicalize()
            .map_err(|_| SaveCredentialsError)?
            != self.target
        {
            return Err(SaveCredentialsError);
        }
        let repository = self.entry.parent().ok_or(SaveCredentialsError)?;
        let directory = self.target.parent().ok_or(SaveCredentialsError)?;
        let owners: &[&std::path::Path] = if directory == repository {
            &[repository]
        } else {
            &[repository, directory]
        };
        let contents =
            self.contents(owners).map_err(|_| SaveCredentialsError)?;
        let replacement = replace_storage(self.format, &contents, fields)?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)
            .map_err(|_| SaveCredentialsError)?;
        temporary
            .write_all(replacement.as_bytes())
            .map_err(|_| SaveCredentialsError)?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|_| SaveCredentialsError)?;
        temporary
            .persist(&self.target)
            .map_err(|_| SaveCredentialsError)?;
        Ok(())
    }
}

struct StorageFields {
    provider: &'static str,
    snapshot: serde_json::Value,
    paths: &'static [&'static [&'static str]],
}

fn replace_storage(
    format: ConfigFormat,
    contents: &str,
    fields: StorageFields,
) -> Result<String, SaveCredentialsError> {
    match format {
        ConfigFormat::Json => {
            let mut document: serde_json::Value =
                serde_json::from_str(contents)
                    .map_err(|_| SaveCredentialsError)?;
            StorageNode::Json(
                document.get_mut("storage").ok_or(SaveCredentialsError)?,
            )
            .apply(&fields)?;
            serde_json::to_string_pretty(&document)
                .map_err(|_| SaveCredentialsError)
        }
        ConfigFormat::Toml => {
            let mut document: toml::Value =
                toml::from_str(contents).map_err(|_| SaveCredentialsError)?;
            StorageNode::Toml(
                document.get_mut("storage").ok_or(SaveCredentialsError)?,
            )
            .apply(&fields)?;
            toml::to_string_pretty(&document).map_err(|_| SaveCredentialsError)
        }
        ConfigFormat::Yaml => {
            let mut document: serde_yaml::Value =
                serde_yaml::from_str(contents)
                    .map_err(|_| SaveCredentialsError)?;
            StorageNode::Yaml(
                document.get_mut("storage").ok_or(SaveCredentialsError)?,
            )
            .apply(&fields)?;
            serde_yaml::to_string(&document).map_err(|_| SaveCredentialsError)
        }
    }
}

enum StorageNode<'a> {
    Json(&'a mut serde_json::Value),
    Toml(&'a mut toml::Value),
    Yaml(&'a mut serde_yaml::Value),
}

impl StorageNode<'_> {
    fn apply(
        &mut self,
        fields: &StorageFields,
    ) -> Result<(), SaveCredentialsError> {
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
            return Err(SaveCredentialsError);
        }
        for path in fields.paths {
            if path.is_empty() || path[0] == "type" {
                return Err(SaveCredentialsError);
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
    ) -> Result<(), SaveCredentialsError> {
        let (&key, tail) = path.split_first().ok_or(SaveCredentialsError)?;
        if !tail.is_empty() {
            if let Some(mut child) = self.child(key, value.is_some())? {
                child.edit(tail, value)?;
            }
            return Ok(());
        }
        match self {
            Self::Json(node) => {
                let map = node.as_object_mut().ok_or(SaveCredentialsError)?;
                if let Some(value) = value {
                    map.insert(key.into(), value.clone());
                } else {
                    map.remove(key);
                }
            }
            Self::Toml(node) => {
                let map = node.as_table_mut().ok_or(SaveCredentialsError)?;
                if let Some(value) = value {
                    map.insert(
                        key.into(),
                        toml::Value::try_from(value)
                            .map_err(|_| SaveCredentialsError)?,
                    );
                } else {
                    map.remove(key);
                }
            }
            Self::Yaml(node) => {
                let map = node.as_mapping_mut().ok_or(SaveCredentialsError)?;
                if let Some(value) = value {
                    map.insert(
                        key.into(),
                        serde_yaml::to_value(value)
                            .map_err(|_| SaveCredentialsError)?,
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
    ) -> Result<Option<StorageNode<'_>>, SaveCredentialsError> {
        match self {
            Self::Json(node) => {
                let map = node.as_object_mut().ok_or(SaveCredentialsError)?;
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
                let map = node.as_table_mut().ok_or(SaveCredentialsError)?;
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
                let map = node.as_mapping_mut().ok_or(SaveCredentialsError)?;
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
    use crate::{ConfigDir, ConfigRepository};
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
                &["binding", "legacy"],
            ],
        }
    }

    #[test]
    fn rejects_ambiguous_names() {
        let directory = TempDir::new().expect("temporary directory");
        std::fs::write(
            directory.path().join("demo.json"),
            r#"{"path":"/mnt/json","storage":{"type":"empty"}}"#,
        )
        .expect("write json");
        std::fs::write(
            directory.path().join("demo.toml"),
            "path = '/mnt/toml'\n[storage]\ntype = 'empty'\n",
        )
        .expect("write toml");
        let repository = ConfigDir::new(directory.path().to_path_buf());
        assert!(repository.find("demo").is_err());
        assert!(repository.list().next().is_none());
    }

    #[test]
    fn rejects_missing_and_path_traversal_names() {
        let directory = TempDir::new().expect("temporary directory");
        let repository = ConfigDir::new(directory.path().to_path_buf());
        assert!(repository.find("missing").is_err());
        assert!(repository.find("../demo").is_err());
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
                "binding": {"client_id": "old", "legacy": "remove", "future": "keep-binding-field"},
                "future": {"keep": true}}
        });
        let mut readable = original.clone();
        readable.as_object_mut().expect("object").remove("driver");
        readable["storage"]
            .as_object_mut()
            .expect("storage")
            .remove("binding");
        std::fs::write(&path, serde_json::to_vec(&readable).expect("encode"))
            .expect("write config");
        let repository = ConfigDir::new(directory.path().to_path_buf());
        let record = repository.find("demo").expect("load record");
        let id = record.record_id.as_deref().expect("record ID");
        std::fs::write(&path, serde_json::to_vec(&original).expect("encode"))
            .expect("add unknown driver");
        repository
            .save_credentials(id, renewed_config(1, "replacement-access"))
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
        assert!(actual["storage"]["binding"].get("legacy").is_none());
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
        let repository = ConfigDir::new(directory.path().to_path_buf());
        let record = repository.find("demo").expect("load record");
        let id = record.record_id.as_deref().expect("record ID");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open file")
            .write_all(b"[driver]\ntype = 'host-extension'\ncache = 9\n")
            .expect("add unknown driver");
        repository
            .save_credentials(id, renewed_config(1, "replacement-access"))
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
            let repository = ConfigDir::new(directory.path().to_path_buf());
            let record = repository.find("demo").expect("load record");
            let id = record.record_id.as_deref().expect("record ID");
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open file")
                .write_all(b"driver:\n  type: host-extension\n  cache: 9\n")
                .expect("add unknown driver");
            repository
                .save_credentials(id, renewed_config(1, "replacement-access"))
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
        let repository = ConfigDir::new(directory.path().to_path_buf());
        let record = repository.find("demo").expect("load record");
        let id = record.record_id.as_deref().expect("record ID");
        std::fs::write(&path, "invalid sentinel-secret")
            .expect("invalidate config");
        let error = repository
            .save_credentials(id, renewed_config(1, "replacement-access"))
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
        let repository = ConfigDir::new(directory.path().to_path_buf());
        let record = repository.find("demo").expect("load record");
        let id = record.record_id.as_deref().expect("record ID");
        let active = repository.record(id).expect("active record");
        let new_save = {
            let writer = active.committed.lock().expect("hold writes");
            let mut old_save = Box::pin(
                repository
                    .save_credentials(id, renewed_config(1, "older-access")),
            );
            let mut new_save =
                Box::pin(repository.save_credentials(
                    id,
                    renewed_config(2, "replacement-access"),
                ));
            assert!(futures::poll!(&mut old_save).is_pending());
            assert!(futures::poll!(&mut new_save).is_pending());
            drop(old_save);
            drop(writer);
            new_save
        };
        new_save.await.expect("newest save succeeds");
        repository
            .save_credentials(id, renewed_config(1, "stale-access"))
            .await
            .expect("stale save is superseded");
        let actual: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(path).expect("read replacement"),
        )
        .expect("json");
        assert_eq!(actual["storage"]["access_token"], "replacement-access");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn record_ids_are_scoped_to_the_repository_and_keep_the_selected_file()
     {
        let first = TempDir::new().expect("first directory");
        let second = TempDir::new().expect("second directory");
        let original = r#"{"path":"/mnt/demo","storage":{"type":"onedrive"}}"#;
        let first_path = first.path().join("demo.json");
        let second_path = second.path().join("demo.json");
        std::fs::write(&first_path, original).expect("first config");
        std::fs::write(&second_path, original).expect("second config");
        let first_repo = ConfigDir::new(first.path().to_path_buf());
        let second_repo = ConfigDir::new(second.path().to_path_buf());
        let first_record = first_repo.find("demo").expect("first record");
        let second_record = second_repo.find("demo").expect("second record");
        let id = first_record.record_id.as_deref().expect("first record ID");
        assert!(second_record.record_id.is_some());
        assert!(
            second_repo
                .save_credentials(id, renewed_config(1, "wrong-file"))
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(&first_path)
                .expect("first remains untouched"),
            original
        );
        assert_eq!(
            std::fs::read_to_string(&second_path)
                .expect("second remains untouched"),
            original
        );

        let independent = first_repo.find("demo").expect("independent load");
        std::fs::write(first.path().join("demo.toml"), "")
            .expect("duplicate name");
        assert!(first_repo.find("demo").is_err());
        first_repo
            .clone()
            .save_credentials(id, renewed_config(2, "right-file"))
            .await
            .expect("already selected record remains usable");
        let saved: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&first_path).expect("read saved record"),
        )
        .expect("json");
        assert_eq!(saved["storage"]["access_token"], "right-file");

        first_repo
            .save_credentials(
                independent.record_id.as_deref().expect("independent ID"),
                renewed_config(1, "independent-access"),
            )
            .await
            .expect("generations are scoped to each load, not the path");
        let saved: serde_json::Value = serde_json::from_slice(
            &std::fs::read(first_path).expect("read independent save"),
        )
        .expect("json");
        assert_eq!(saved["storage"]["access_token"], "independent-access");
    }
}
