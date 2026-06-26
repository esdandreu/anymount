use std::path::{Path, PathBuf};

use crate::domain::Config;
use crate::domain::config_repository::ConfigRepository;

/// A configuration repository that reads configuration from files within a
/// folder. The name of the file determines the name of the configuration.
#[derive(Debug, Clone)]
pub struct ConfigDir {
    path: PathBuf,
}

impl ConfigDir {
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
            Err(_) => ConfigFileIter::Empty,
        }
    }
}

pub enum ConfigFileIter {
    Entries(std::fs::ReadDir),
    Empty,
}

impl Iterator for ConfigFileIter {
    type Item = (String, Config);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Entries(entries) => entries.find_map(|entry| match entry {
                Ok(entry) => match read_config(entry.path()) {
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

fn read_config(path: PathBuf) -> Result<(String, Config), ReadConfigError> {
    if !path.is_file() {
        return Err(ReadConfigError::NotAConfigFile);
    }
    let name = path
        .file_stem()
        .and_then(|file_stem| file_stem.to_str())
        .ok_or_else(|| ReadConfigError::NotAConfigFile)?
        .to_owned();
    let format = ConfigFormat::from_path(&path)
        .ok_or_else(|| ReadConfigError::NotAConfigFile)?;
    let contents = std::fs::read_to_string(&path)
        .map_err(|source| ReadConfigError::CannotReadFile { source })?;
    let config = format
        .deserialize(&contents)
        .map_err(|message| ReadConfigError::CannotDeserialize { message })?;
    Ok((name, config))
}

#[derive(Debug, thiserror::Error)]
enum ReadConfigError {
    #[error("not a configuration file")]
    NotAConfigFile,

    #[error("failed to read file: {source}")]
    CannotReadFile { source: std::io::Error },

    #[error("failed to deserialize configuration: {message}")]
    CannotDeserialize { message: String },
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

    fn deserialize(&self, contents: &str) -> Result<Config, String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::storage::ConnectError;
    use tempfile::TempDir;
    use tracing_test::traced_test;

    #[test]
    fn reads_toml_config_file() {
        let dir = TempDir::new().expect("create temp dir");
        std::fs::write(
            &dir.path().join("demo.toml"),
            r#"
[storage]
type = "cannot-connect"
error_message = "toml test"
"#,
        )
        .expect("write toml config");
        let config = ConfigDir::new(dir.path().to_path_buf());
        let mounts = config.list().collect::<Vec<_>>();
        let (name, config) = mounts.into_iter().next().expect("read config");
        assert_eq!(name, "demo");
        match config.storage.connect() {
            Ok(_) => panic!("should not connect"),
            Err(ConnectError::CannotConnect { message }) => {
                assert_eq!(message, "toml test");
            }
        }
    }

    #[test]
    fn reads_json_config_file() {
        let dir = TempDir::new().expect("create temp dir");
        std::fs::write(
            &dir.path().join("demo.json"),
            r#"{
        "storage": {
            "type": "cannot-connect",
            "error_message": "json test"
        }
}"#,
        )
        .expect("write json config");

        let config = ConfigDir::new(dir.path().to_path_buf());
        let mounts = config.list().collect::<Vec<_>>();
        let (name, config) = mounts.into_iter().next().expect("read config");
        assert_eq!(name, "demo");
        match config.storage.connect() {
            Ok(_) => panic!("should not connect"),
            Err(ConnectError::CannotConnect { message }) => {
                assert_eq!(message, "json test");
            }
        }
    }

    #[test]
    fn reads_yaml_config_file() {
        let dir = TempDir::new().expect("create temp dir");
        std::fs::write(
            &dir.path().join("demo.yaml"),
            r#"
storage:
  type: cannot-connect
  error_message: yaml test
"#,
        )
        .expect("write yaml config");

        let config = ConfigDir::new(dir.path().to_path_buf());
        let mounts = config.list().collect::<Vec<_>>();
        let (name, config) = mounts.into_iter().next().expect("read config");
        assert_eq!(name, "demo");
        match config.storage.connect() {
            Ok(_) => panic!("should not connect"),
            Err(ConnectError::CannotConnect { message }) => {
                assert_eq!(message, "yaml test");
            }
        }
    }

    #[test]
    #[traced_test]
    fn warns_config_errors() {
        let dir = TempDir::new().expect("create temp dir");
        std::fs::write(&dir.path().join("demo.toml"), r#"not toml"#)
            .expect("write toml config");
        let config = ConfigDir::new(dir.path().to_path_buf());
        let mounts = config.list().collect::<Vec<_>>();
        assert!(mounts.is_empty());
        assert!(logs_contain("failed to read config"));
    }

    #[test]
    fn read_config_not_a_config_file() {
        let dir = TempDir::new().expect("create temp dir");
        let path = dir.path().join("demo.txt");
        std::fs::write(&path, "not a config").expect("write unknown config");

        assert!(matches!(
            read_config(path),
            Err(ReadConfigError::NotAConfigFile)
        ));
    }

    #[test]
    fn read_config_cannot_deserialize() {
        let dir = TempDir::new().expect("create temp dir");
        let path = dir.path().join("demo.toml");
        std::fs::write(
            &dir.path().join("demo.toml"),
            r#"
[storage]
type = "unknown"
key = "value"
"#,
        )
        .expect("write toml config");
        match read_config(path) {
            Err(ReadConfigError::CannotDeserialize { message }) => {
                assert!(message.starts_with("TOML parse error"));
                assert!(message.contains("unknown variant"));
            }
            Err(other) => panic!("expected deserialize error, got {other:?}"),
            Ok(_) => panic!("expected failure"),
        }
    }
}
