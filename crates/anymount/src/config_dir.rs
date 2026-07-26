use std::path::PathBuf;

use crate::domain::{Config, ConfigRepository};

mod read_config;

use read_config::{ReadConfigError, read_config};

/// A configuration repository that reads configuration from files within a
/// folder. The name of the file determines the name of the configuration.
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
    type Item = Config;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Entries(entries) => entries.find_map(|entry| match entry {
                Ok(entry) => match read_config(&entry.path()) {
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
    use tempfile::TempDir;
    use tracing_test::traced_test;

    #[test]
    fn reads_toml_config_file() {
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
        assert_eq!(config.name, "demo");
        match config.storage.connect() {
            Ok(_) => panic!("should not connect"),
            Err(error) => {
                assert_eq!(error.message, "toml test");
            }
        }
    }

    #[test]
    fn reads_json_config_file() {
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
        assert_eq!(config.name, "demo");
        match config.storage.connect() {
            Ok(_) => panic!("should not connect"),
            Err(error) => {
                assert_eq!(error.message, "json test");
            }
        }
    }

    #[test]
    fn reads_yaml_config_file() {
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
        assert_eq!(config.name, "demo");
        match config.storage.connect() {
            Ok(_) => panic!("should not connect"),
            Err(error) => {
                assert_eq!(error.message, "yaml test");
            }
        }
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
