use crate::{ProviderConfiguration, ProvidersConfiguration, StorageConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{fs, io};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot read config dir {path}: {source}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("cannot read config {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("invalid config {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("cannot create config dir {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("cannot write config {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("cannot remove config {path}: {source}")]
    Remove {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("cannot serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),

    #[error("non-utf8 filename: {path}")]
    NonUtf8FileName { path: PathBuf },
}

pub type Result<T> = std::result::Result<T, Error>;

fn default_true() -> bool {
    true
}

/// Optional OpenTelemetry export settings for a named provider.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelemetryFileConfig {
    #[serde(default)]
    pub otlp: Option<OtlpTelemetryConfig>,
}

/// OTLP exporter settings under `[telemetry.otlp]` in a provider `.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpTelemetryConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub protocol: Option<OtlpTransport>,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub resource_attributes: Option<HashMap<String, String>>,
}

/// OTLP wire transport (only HTTP/protobuf is implemented today).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum OtlpTransport {
    #[default]
    #[serde(rename = "http/protobuf")]
    HttpProtobuf,
    #[serde(rename = "grpc")]
    Grpc,
}

/// Single provider entry stored as `<name>.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderFileConfig {
    pub path: PathBuf,
    pub storage: StorageConfig,
    #[serde(default)]
    pub telemetry: TelemetryFileConfig,
}

impl ProviderConfiguration for ProviderFileConfig {
    fn path(&self) -> PathBuf {
        self.path.clone()
    }

    fn storage_config(&self) -> StorageConfig {
        self.storage.clone()
    }
}

/// Collection of providers loaded from the config directory.
#[derive(Debug, Clone)]
pub struct Config {
    pub providers: Vec<ProviderFileConfig>,
}

impl ProvidersConfiguration for Config {
    fn providers(&self) -> Vec<&impl ProviderConfiguration> {
        self.providers.iter().collect()
    }
}

/// Handle to the configuration directory.
#[derive(Debug, Clone)]
pub struct ConfigDir {
    dir: PathBuf,
}

impl Default for ConfigDir {
    fn default() -> Self {
        Self::new(default_config_dir())
    }
}

impl ConfigDir {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// List the names of all configured providers (filenames without
    /// the `.toml` extension).
    pub fn list(&self) -> Result<Vec<String>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(e) => {
                return Err(Error::ReadDir {
                    path: self.dir.clone(),
                    source: e,
                });
            }
        };
        let mut names: Vec<String> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| Error::ReadDir {
                path: self.dir.clone(),
                source,
            })?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                if let Some(stem) = path.file_stem() {
                    names.push(
                        stem.to_str()
                            .ok_or_else(|| Error::NonUtf8FileName { path: path.clone() })?
                            .to_owned(),
                    );
                }
            }
        }
        names.sort();
        Ok(names)
    }

    /// Walk configured providers in sorted name order.
    ///
    /// The outer [`Result`] fails only when the directory cannot be listed. Each
    /// item pairs the provider name with the result of reading its `.toml` file.
    pub fn each_provider(
        &self,
    ) -> Result<impl Iterator<Item = (String, Result<ProviderFileConfig>)>> {
        let names = self.list()?;
        let this = self.clone();
        Ok(names.into_iter().map(move |name| {
            let loaded = this.read(&name);
            (name, loaded)
        }))
    }

    /// Read a single provider config by name.
    pub fn read(&self, name: &str) -> Result<ProviderFileConfig> {
        let path = self.file_path(name);
        let contents = fs::read_to_string(&path).map_err(|source| Error::Read {
            path: path.clone(),
            source,
        })?;
        toml::from_str(&contents).map_err(|source| Error::Parse { path, source })
    }

    /// Write (create or overwrite) a provider config.
    pub fn write(&self, name: &str, config: &ProviderFileConfig) -> Result<()> {
        fs::create_dir_all(&self.dir).map_err(|source| Error::CreateDir {
            path: self.dir.clone(),
            source,
        })?;
        let path = self.file_path(name);
        let contents = toml::to_string_pretty(config)?;
        fs::write(&path, contents).map_err(|source| Error::Write { path, source })
    }

    /// Remove a provider config file.
    pub fn remove(&self, name: &str) -> Result<()> {
        let path = self.file_path(name);
        fs::remove_file(&path).map_err(|source| Error::Remove { path, source })
    }

    /// Load all provider configs from the directory.
    pub fn load_all(&self) -> Result<Config> {
        let providers = self
            .each_provider()?
            .map(|(_name, loaded)| loaded)
            .collect::<std::result::Result<Vec<_>, Error>>()?;
        Ok(Config { providers })
    }

    fn file_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.toml"))
    }
}

/// Platform-dependent default config directory
/// (`$XDG_CONFIG_HOME/anymount`, `~/Library/Application
/// Support/anymount`, `%APPDATA%\anymount`, etc.).
pub fn default_config_dir() -> PathBuf {
    dirs::config_dir()
        .expect("no platform config directory available")
        .join("anymount")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_config_dir() -> (tempfile::TempDir, ConfigDir) {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        (tmp, cd)
    }

    fn local_config() -> ProviderFileConfig {
        ProviderFileConfig {
            path: PathBuf::from("/mnt/local"),
            storage: StorageConfig::Local {
                root: PathBuf::from("/data"),
            },
            telemetry: TelemetryFileConfig::default(),
        }
    }

    fn onedrive_config() -> ProviderFileConfig {
        ProviderFileConfig {
            path: PathBuf::from("/mnt/onedrive"),
            storage: StorageConfig::OneDrive {
                root: PathBuf::from("/"),
                endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
                access_token: None,
                refresh_token: Some("rt_value".to_owned()),
                client_id: Some("cid".to_owned()),
                token_expiry_buffer_secs: Some(60),
            },
            telemetry: TelemetryFileConfig::default(),
        }
    }

    #[test]
    fn list_empty_when_dir_missing() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let cd = ConfigDir::new(tmp.path().join("nonexistent"));
        assert_eq!(cd.list().expect("list failed"), Vec::<String>::new());
    }

    #[test]
    fn write_and_read_local() {
        let (_tmp, cd) = tmp_config_dir();
        let cfg = local_config();
        cd.write("mylocal", &cfg).expect("write failed");
        let loaded = cd.read("mylocal").expect("read failed");
        assert_eq!(loaded.path, cfg.path);
    }

    #[test]
    fn write_and_read_otlp_telemetry_section() {
        let (_tmp, cd) = tmp_config_dir();
        let cfg = ProviderFileConfig {
            path: PathBuf::from("/mnt/otel"),
            storage: StorageConfig::Local {
                root: PathBuf::from("/data"),
            },
            telemetry: TelemetryFileConfig {
                otlp: Some(OtlpTelemetryConfig {
                    enabled: true,
                    endpoint: Some("http://localhost:4318".to_owned()),
                    protocol: Some(OtlpTransport::HttpProtobuf),
                    headers: None,
                    resource_attributes: None,
                }),
            },
        };
        cd.write("otel", &cfg).expect("write failed");
        let loaded = cd.read("otel").expect("read failed");
        let otlp = loaded.telemetry.otlp.expect("otlp section");
        assert!(otlp.enabled);
        assert_eq!(otlp.endpoint.as_deref(), Some("http://localhost:4318"));
    }

    #[test]
    fn write_and_read_onedrive() {
        let (_tmp, cd) = tmp_config_dir();
        let cfg = onedrive_config();
        cd.write("myod", &cfg).expect("write failed");
        let loaded = cd.read("myod").expect("read failed");
        assert_eq!(loaded.path, cfg.path);
        if let StorageConfig::OneDrive {
            refresh_token,
            client_id,
            ..
        } = &loaded.storage
        {
            assert_eq!(refresh_token.as_deref(), Some("rt_value"));
            assert_eq!(client_id.as_deref(), Some("cid"));
        } else {
            panic!("expected OneDrive config");
        }
    }

    #[test]
    fn list_returns_sorted_names() {
        let (_tmp, cd) = tmp_config_dir();
        cd.write("bravo", &local_config()).expect("write failed");
        cd.write("alpha", &local_config()).expect("write failed");
        let names = cd.list().expect("list failed");
        assert_eq!(names, vec!["alpha", "bravo"]);
    }

    #[test]
    fn each_provider_yields_sorted_name_and_config() {
        let (_tmp, cd) = tmp_config_dir();
        cd.write("bravo", &local_config()).expect("write failed");
        cd.write("alpha", &onedrive_config()).expect("write failed");
        let entries: Vec<_> = cd.each_provider().expect("each_provider").collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "alpha");
        assert!(entries[0].1.as_ref().expect("alpha load").storage.label() == "onedrive");
        assert_eq!(entries[1].0, "bravo");
        assert!(entries[1].1.as_ref().expect("bravo load").storage.label() == "local");
    }

    #[test]
    fn remove_deletes_file() {
        let (_tmp, cd) = tmp_config_dir();
        cd.write("gone", &local_config()).expect("write failed");
        cd.remove("gone").expect("remove failed");
        assert!(cd.read("gone").is_err());
    }

    #[test]
    fn load_all_returns_all_providers() {
        let (_tmp, cd) = tmp_config_dir();
        cd.write("a", &local_config()).expect("write failed");
        cd.write("b", &onedrive_config()).expect("write failed");
        let config = cd.load_all().expect("load_all failed");
        assert_eq!(config.providers.len(), 2);
    }

    #[test]
    fn read_nonexistent_returns_error() {
        let (_tmp, cd) = tmp_config_dir();
        assert!(cd.read("nope").is_err());
    }

    #[test]
    fn read_nonexistent_returns_read_error() {
        let (_tmp, cd) = tmp_config_dir();
        let err = cd.read("nope").expect_err("read should fail");

        assert!(matches!(err, Error::Read { .. }));
    }

    #[test]
    fn read_invalid_toml_returns_parse_error() {
        let (_tmp, cd) = tmp_config_dir();
        std::fs::write(cd.dir().join("broken.toml"), "path = [").expect("seed invalid toml");

        let err = cd.read("broken").expect_err("read should fail");
        assert!(matches!(err, Error::Parse { .. }));
    }

    #[test]
    fn remove_nonexistent_returns_error() {
        let (_tmp, cd) = tmp_config_dir();
        assert!(cd.remove("nope").is_err());
    }

    #[test]
    fn roundtrip_serialization_preserves_none_fields() {
        let (_tmp, cd) = tmp_config_dir();
        let cfg = ProviderFileConfig {
            path: PathBuf::from("/mnt/od"),
            storage: StorageConfig::OneDrive {
                root: PathBuf::from("/"),
                endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
                access_token: None,
                refresh_token: None,
                client_id: None,
                token_expiry_buffer_secs: None,
            },
            telemetry: TelemetryFileConfig::default(),
        };
        cd.write("sparse", &cfg).expect("write failed");
        let loaded = cd.read("sparse").expect("read failed");
        if let StorageConfig::OneDrive {
            access_token,
            refresh_token,
            client_id,
            token_expiry_buffer_secs,
            ..
        } = &loaded.storage
        {
            assert!(access_token.is_none());
            assert!(refresh_token.is_none());
            assert!(client_id.is_none());
            assert!(token_expiry_buffer_secs.is_none());
        } else {
            panic!("expected OneDrive config");
        }
    }

    #[test]
    fn provider_file_config_implements_provider_configuration() {
        let cfg = local_config();
        assert_eq!(
            ProviderConfiguration::path(&cfg),
            PathBuf::from("/mnt/local")
        );
    }
}
