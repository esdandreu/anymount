mod patch;

use std::fs::File;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::domain::DriverConfig;
use crate::domain::{Config, MountConfig, StorageConfig};

#[derive(Debug)]
pub struct ConfigFile {
    pub entry: PathBuf,
    pub target: PathBuf,
    format: ConfigFormat,
}

impl ConfigFile {
    pub fn resolve(entry: PathBuf) -> Option<Self> {
        let format = ConfigFormat::from_path(&entry)?;
        let target = entry
            .canonicalize()
            .map_err(ReadConfigError::from)
            .inspect_err(|error| error.warn(&entry))
            .ok()?;
        Some(Self {
            entry,
            target,
            format,
        })
    }

    pub fn read(&self) -> Result<Config, ReadConfigError> {
        let name = self
            .entry
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or(ReadConfigError::NotAConfigFile)?;
        let directory =
            self.entry.parent().ok_or(ReadConfigError::NotAConfigFile)?;
        let contents = self.contents(&[directory])?;
        let content: ConfigFileContent = match self.format {
            ConfigFormat::Toml => toml::from_str(&contents).ok(),
            ConfigFormat::Json => serde_json::from_str(&contents).ok(),
            ConfigFormat::Yaml => serde_yaml::from_str(&contents).ok(),
        }
        .ok_or(ReadConfigError::CannotDeserialize)?;
        Ok(Config {
            mount: MountConfig {
                name: name.to_owned(),
                path: content.path,
                storage: content.storage,
            },
            driver: content.driver,
        })
    }

    fn contents(&self, owners: &[&Path]) -> Result<String, ReadConfigError> {
        let mut contents = String::new();
        open_config_file(&self.target, owners)?
            .read_to_string(&mut contents)?;
        Ok(contents)
    }
}

fn open_config_file(
    path: &Path,
    _owner_directories: &[&Path],
) -> Result<File, ReadConfigError> {
    let file = File::open(path)?;

    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(ReadConfigError::NotAConfigFile);
    }

    #[cfg(unix)]
    for directory in _owner_directories {
        let directory_owner = directory.metadata()?.uid();
        if metadata.uid() != directory_owner {
            return Err(ReadConfigError::OwnerMismatch {
                file_owner: metadata.uid(),
                directory_owner,
            });
        }
    }

    Ok(file)
}

#[derive(serde::Deserialize)]
struct ConfigFileContent {
    path: PathBuf,
    storage: Box<dyn StorageConfig>,
    driver: Option<Box<dyn DriverConfig>>,
}
#[derive(Clone, Copy, Debug)]
pub enum ConfigFormat {
    Toml,
    Json,
    Yaml,
}

impl ConfigFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "toml" => Some(Self::Toml),
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReadConfigError {
    #[error("not a configuration file")]
    NotAConfigFile,

    #[error("failed to read file: {source}")]
    CannotReadFile {
        #[from]
        source: std::io::Error,
    },

    #[cfg(unix)]
    #[error(
        "configuration file owner {file_owner} differs from directory owner \
         {directory_owner}"
    )]
    OwnerMismatch {
        file_owner: u32,
        directory_owner: u32,
    },

    #[error("failed to deserialize configuration: invalid configuration")]
    CannotDeserialize,

    #[error("failed to initialize storage connector")]
    CannotInitializeConnector,
}

impl ReadConfigError {
    pub fn warn(&self, path: &Path) {
        if !matches!(self, Self::NotAConfigFile) {
            tracing::warn!(error = %self, path = %path.display(), "failed to read config");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::{
        ConfigDir,
        domain::{ConfigRepository, CredentialUpdate},
    };
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[test]
    fn read_config_not_a_config_file() {
        let dir = TempDir::new().expect("create temp dir");
        let path = dir.path().join("demo.txt");
        std::fs::write(&path, "not a config").expect("write config");

        assert!(ConfigFile::resolve(path).is_none());
    }

    #[test]
    fn read_config_cannot_deserialize() {
        let dir = TempDir::new().expect("create temp dir");
        let path = dir.path().join("demo.toml");
        std::fs::write(
            &path,
            r#"
path = "/mnt/test"
[storage]
type = "unknown"
key = "value"
"#,
        )
        .expect("write config");

        let file = ConfigFile::resolve(path).expect("resolve config");
        assert!(matches!(
            file.read(),
            Err(ReadConfigError::CannotDeserialize)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn reads_config_with_writable_permissions() {
        let dir = TempDir::new().expect("create temp dir");
        let path = dir.path().join("demo.toml");
        std::fs::write(
            &path,
            r#"
path = "/mnt/test"
[storage]
type = "cannot-connect"
error_message = "writable"
"#,
        )
        .expect("write config");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666))
            .expect("set writable permissions");

        assert!(ConfigDir::new(dir.path().into()).find("demo").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn reads_symlinked_config() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().expect("create temp dir");
        let target = dir.path().join("target.toml");
        let link = dir.path().join("demo.toml");
        std::fs::write(
            &target,
            r#"
path = "/mnt/test"
[storage]
type = "cannot-connect"
error_message = "symlink"
"#,
        )
        .expect("write config");
        symlink(&target, &link).expect("create symlink");

        assert!(ConfigDir::new(dir.path().into()).find("demo").is_ok());
    }
    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn saves_symlinked_toml_with_extensionless_target() {
        let directory = TempDir::new().expect("temporary directory");
        let target = directory.path().join("credentials");
        let link = directory.path().join("drive.toml");
        std::fs::write(
            &target,
            "path = '/mnt/demo'\n[storage]\ntype = 'onedrive'\naccess_token = 'old'\n",
        )
        .expect("write config");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");
        let repository = ConfigDir::new(directory.path().to_path_buf());
        let record = repository.find("drive").expect("load record");
        let storage: crate::storages::onedrive::OneDriveConfig = serde_json::from_value(
            serde_json::json!({"access_token":"replacement-access", "refresh_token":"replacement-refresh"})
        ).expect("config");
        repository
            .save_credentials(
                record.record_id.as_deref().expect("record ID"),
                CredentialUpdate {
                    generation: 1,
                    config: Box::new(storage),
                    storage_type: "onedrive",
                    fields: &[&["access_token"], &["refresh_token"]],
                },
            )
            .await
            .expect("save credentials");

        assert!(
            std::fs::symlink_metadata(&link)
                .expect("link metadata")
                .file_type()
                .is_symlink()
        );
        let actual: toml::Value = toml::from_str(
            &std::fs::read_to_string(&target).expect("read target"),
        )
        .expect("toml format retained");
        assert_eq!(
            actual["storage"]["refresh_token"].as_str(),
            Some("replacement-refresh")
        );
        assert!(repository.find("drive").is_ok());

        let other = directory.path().join("other-credentials");
        let original = "path = '/mnt/other'\n[storage]\ntype = 'onedrive'\n";
        std::fs::write(&other, original).expect("write other record");
        std::fs::remove_file(&link).expect("remove link");
        std::os::unix::fs::symlink(&other, &link).expect("retarget link");
        let storage: crate::storages::onedrive::OneDriveConfig =
            serde_json::from_str("{}").expect("config without a refresh token");
        let update = CredentialUpdate {
            generation: 2,
            config: Box::new(storage),
            storage_type: "onedrive",
            fields: &[&["refresh_token"]],
        };
        assert!(
            repository
                .save_credentials(
                    record.record_id.as_deref().expect("record ID"),
                    update
                )
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(other).expect("read other"),
            original
        );
        let retained: toml::Value = toml::from_str(
            &std::fs::read_to_string(target).expect("read original target"),
        )
        .expect("toml");
        assert_eq!(
            retained["storage"]["refresh_token"].as_str(),
            Some("replacement-refresh")
        );
        assert!(repository.find("drive").is_ok());
    }
    #[test]
    fn parse_errors_do_not_expose_configuration_secrets() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("drive.toml");
        std::fs::write(&path, "access_token = sentinel-secret")
            .expect("write invalid configuration");
        let file = ConfigFile::resolve(path).expect("resolve config");
        let error = file.read().err().expect("parse failure");
        assert!(!format!("{error:?} {error}").contains("sentinel-secret"));
    }
    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn saved_credentials_have_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("demo.json");
        std::fs::write(
            &path,
            r#"{"path":"/mnt/demo","storage":{"type":"onedrive"}}"#,
        )
        .expect("write config");
        let storage = serde_json::from_value::<
            crate::storages::onedrive::OneDriveConfig,
        >(serde_json::json!({
            "access_token": "replacement-access",
            "root": "documents",
            "endpoint": "https://graph.microsoft.com/v1.0"
        }))
        .expect("configuration");
        let repository = ConfigDir::new(directory.path().to_path_buf());
        let record = repository.find("demo").expect("load record");
        repository
            .save_credentials(
                record.record_id.as_deref().expect("record ID"),
                CredentialUpdate {
                    generation: 1,
                    config: Box::new(storage),
                    storage_type: "onedrive",
                    fields: &[&["access_token"]],
                },
            )
            .await
            .expect("save credentials");
        assert_eq!(
            std::fs::metadata(path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
