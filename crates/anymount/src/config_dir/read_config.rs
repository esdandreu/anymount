use std::fs::File;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::domain::driver::DriverConfig;
use crate::domain::{Config, StorageConfig};

pub fn read_config(path: &Path) -> Result<Config, ReadConfigError> {
    let name = path
        .file_stem()
        .and_then(|file_stem| file_stem.to_str())
        .ok_or(ReadConfigError::NotAConfigFile)?
        .to_owned();
    let content = read_config_file(path)?;
    Ok(Config {
        name,
        path: content.path,
        storage: content.storage,
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
    format
        .deserialize(&contents)
        .map_err(|message| ReadConfigError::CannotDeserialize { message })
}

fn open_config_file(path: &Path) -> Result<File, ReadConfigError> {
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

    fn deserialize(&self, contents: &str) -> Result<ConfigFileContent, String> {
        match self {
            Self::Toml => {
                toml::from_str(contents).map_err(|error| error.to_string())
            }
            Self::Json => serde_json::from_str(contents)
                .map_err(|error| error.to_string()),
            Self::Yaml => serde_yaml::from_str(contents)
                .map_err(|error| error.to_string()),
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
                assert!(message.starts_with("TOML parse error"));
                assert!(message.contains("unknown variant"));
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
}
