use std::fs::File;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::domain::DriverConfig;
use crate::domain::{Config, MountConfig, StorageConfig};

pub fn read_config(path: &Path) -> Result<Config, ReadConfigError> {
    let name = path
        .file_stem()
        .and_then(|file_stem| file_stem.to_str())
        .ok_or(ReadConfigError::NotAConfigFile)?
        .to_owned();
    let content = read_config_file(path)?;
    Ok(Config {
        mount: MountConfig {
            name,
            path: content.path,
            storage: content.storage,
        },
        driver: content.driver,
    })
}

fn read_config_file(path: &Path) -> Result<ConfigFileContent, ReadConfigError> {
    let format =
        ConfigFormat::from_path(path).ok_or(ReadConfigError::NotAConfigFile)?;
    let mut file = open_config_file(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|source| ReadConfigError::CannotReadFile { source })?;
    format.deserialize(&contents).map_err(|_| {
        ReadConfigError::CannotDeserialize {
            message: "invalid configuration".to_owned(),
        }
    })
}

pub fn open_config_file(path: &Path) -> Result<File, ReadConfigError> {
    let file = File::open(path)
        .map_err(|source| ReadConfigError::CannotReadFile { source })?;

    let metadata = file
        .metadata()
        .map_err(|source| ReadConfigError::CannotReadFile { source })?;
    if !metadata.is_file() {
        return Err(ReadConfigError::NotAConfigFile);
    }

    #[cfg(unix)]
    {
        let directory_metadata = path
            .parent()
            .ok_or(ReadConfigError::NotAConfigFile)?
            .metadata()
            .map_err(|source| ReadConfigError::CannotReadFile { source })?;
        if metadata.uid() != directory_metadata.uid() {
            return Err(ReadConfigError::OwnerMismatch {
                file_owner: metadata.uid(),
                directory_owner: directory_metadata.uid(),
            });
        }
    }

    Ok(file)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ConfigFileContent {
    path: PathBuf,
    storage: Box<dyn StorageConfig>,
    driver: Option<Box<dyn DriverConfig>>,
}

enum ConfigFormat {
    Toml,
    Json,
    Yaml,
}

impl ConfigFormat {
    fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "toml" => Some(Self::Toml),
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            _ => None,
        }
    }

    fn deserialize(&self, contents: &str) -> Result<ConfigFileContent, ()> {
        match self {
            Self::Toml => toml::from_str(contents).map_err(|_| ()),
            Self::Json => serde_json::from_str(contents).map_err(|_| ()),
            Self::Yaml => serde_yaml::from_str(contents).map_err(|_| ()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReadConfigError {
    #[error("not a configuration file")]
    NotAConfigFile,

    #[error("failed to read file: {source}")]
    CannotReadFile { source: std::io::Error },

    #[cfg(unix)]
    #[error(
        "configuration file owner {file_owner} differs from directory owner \
         {directory_owner}"
    )]
    OwnerMismatch {
        file_owner: u32,
        directory_owner: u32,
    },

    #[error("failed to deserialize configuration: {message}")]
    CannotDeserialize { message: String },

    #[error("failed to initialize configuration session")]
    CannotInitializeSession,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::{
        config_dir::ConfigFilePersistence,
        domain::{ConfigurationPersistence, CredentialUpdate},
    };
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[test]
    fn read_config_not_a_config_file() {
        let dir = TempDir::new().expect("create temp dir");
        let path = dir.path().join("demo.txt");
        std::fs::write(&path, "not a config").expect("write config");

        assert!(matches!(
            read_config_file(&path),
            Err(ReadConfigError::NotAConfigFile)
        ));
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

        match read_config_file(&path) {
            Err(ReadConfigError::CannotDeserialize { message }) => {
                assert_eq!(message, "invalid configuration");
            }
            Err(other) => panic!("expected deserialize error, got {other:?}"),
            Ok(_) => panic!("expected failure"),
        }
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

        assert!(read_config_file(&path).is_ok());
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

        assert!(read_config_file(&link).is_ok());
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
        let persistence =
            ConfigFilePersistence::open(directory.path(), "drive")
                .expect("open record");
        let storage: crate::storages::onedrive::OneDriveConfig = serde_json::from_value(
            serde_json::json!({"access_token":"replacement-access", "refresh_token":"replacement-refresh"})
        ).expect("config");
        persistence
            .save(&CredentialUpdate {
                generation: 1,
                config: Box::new(storage),
                storage_type: "onedrive",
                fields: &[&["access_token"], &["refresh_token"]],
            })
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
        assert!(read_config(&link).is_ok());
    }
    #[test]
    fn parse_errors_do_not_expose_configuration_secrets() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("drive.toml");
        std::fs::write(&path, "access_token = sentinel-secret")
            .expect("write invalid configuration");
        let error = match read_config_file(&path) {
            Err(error) => error,
            Ok(_) => panic!("expected parse failure"),
        };
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
        ConfigFilePersistence::open(directory.path(), "demo")
            .expect("open record")
            .save(&CredentialUpdate {
                generation: 1,
                config: Box::new(storage),
                storage_type: "onedrive",
                fields: &[&["access_token"]],
            })
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
