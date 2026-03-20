use crate::domain::driver::{
    Driver, OtlpSpec as DomainOtlpSpec, OtlpTransport as DomainOtlpTransport, StorageSpec,
    TelemetrySpec,
};
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

    #[error("invalid driver spec {name}: {source}")]
    InvalidDriverSpec {
        name: String,
        #[source]
        source: crate::domain::driver::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StorageConfig {
    Local {
        root: PathBuf,
    },
    OneDrive {
        root: PathBuf,
        endpoint: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        access_token: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        refresh_token: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        client_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        token_expiry_buffer_secs: Option<u64>,
    },
}

impl StorageConfig {
    /// Short label for CLI and status output (`local`, `onedrive`, ...).
    pub fn label(&self) -> &'static str {
        match self {
            Self::Local { .. } => "local",
            Self::OneDrive { .. } => "onedrive",
        }
    }
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

/// Single driver entry stored as `<name>.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverFileConfig {
    pub path: PathBuf,
    pub storage: StorageConfig,
    #[serde(default)]
    pub telemetry: TelemetryFileConfig,
}

impl DriverFileConfig {
    fn into_spec(self, name: String) -> Driver {
        Driver {
            name,
            path: self.path,
            storage: self.storage.into(),
            telemetry: self.telemetry.into(),
        }
    }

    fn from_spec(spec: &Driver) -> Self {
        Self {
            path: spec.path.clone(),
            storage: spec.storage.clone().into(),
            telemetry: spec.telemetry.clone().into(),
        }
    }
}

impl From<StorageConfig> for StorageSpec {
    fn from(value: StorageConfig) -> Self {
        match value {
            StorageConfig::Local { root } => Self::Local { root },
            StorageConfig::OneDrive {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            } => Self::OneDrive {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            },
        }
    }
}

impl From<StorageSpec> for StorageConfig {
    fn from(value: StorageSpec) -> Self {
        match value {
            StorageSpec::Local { root } => Self::Local { root },
            StorageSpec::OneDrive {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            } => Self::OneDrive {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            },
        }
    }
}

impl From<TelemetryFileConfig> for TelemetrySpec {
    fn from(value: TelemetryFileConfig) -> Self {
        Self {
            otlp: value.otlp.map(Into::into),
        }
    }
}

impl From<TelemetrySpec> for TelemetryFileConfig {
    fn from(value: TelemetrySpec) -> Self {
        Self {
            otlp: value.otlp.map(Into::into),
        }
    }
}

impl From<OtlpTelemetryConfig> for DomainOtlpSpec {
    fn from(value: OtlpTelemetryConfig) -> Self {
        Self {
            enabled: value.enabled,
            endpoint: value.endpoint,
            protocol: value.protocol.map(Into::into),
            headers: value.headers,
            resource_attributes: value.resource_attributes,
        }
    }
}

impl From<DomainOtlpSpec> for OtlpTelemetryConfig {
    fn from(value: DomainOtlpSpec) -> Self {
        Self {
            enabled: value.enabled,
            endpoint: value.endpoint,
            protocol: value.protocol.map(Into::into),
            headers: value.headers,
            resource_attributes: value.resource_attributes,
        }
    }
}

impl From<OtlpTransport> for DomainOtlpTransport {
    fn from(value: OtlpTransport) -> Self {
        match value {
            OtlpTransport::HttpProtobuf => Self::HttpProtobuf,
            OtlpTransport::Grpc => Self::Grpc,
        }
    }
}

impl From<DomainOtlpTransport> for OtlpTransport {
    fn from(value: DomainOtlpTransport) -> Self {
        match value {
            DomainOtlpTransport::HttpProtobuf => Self::HttpProtobuf,
            DomainOtlpTransport::Grpc => Self::Grpc,
        }
    }
}

/// Collection of drivers loaded from the config directory.
#[derive(Debug, Clone)]
pub struct Config {
    pub drivers: Vec<DriverFileConfig>,
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

    /// Walk configured drivers in sorted name order.
    ///
    /// The outer [`Result`] fails only when the directory cannot be listed. Each
    /// item pairs the driver name with the result of reading its `.toml` file.
    pub fn each_driver(&self) -> Result<impl Iterator<Item = (String, Result<DriverFileConfig>)>> {
        let names = self.list()?;
        let this = self.clone();
        Ok(names.into_iter().map(move |name| {
            let loaded = this.read(&name);
            (name, loaded)
        }))
    }

    /// Read a single driver config by name.
    pub fn read(&self, name: &str) -> Result<DriverFileConfig> {
        let path = self.file_path(name);
        let contents = fs::read_to_string(&path).map_err(|source| Error::Read {
            path: path.clone(),
            source,
        })?;
        toml::from_str(&contents).map_err(|source| Error::Parse { path, source })
    }

    /// Write (create or overwrite) a driver config.
    pub fn write(&self, name: &str, config: &DriverFileConfig) -> Result<()> {
        fs::create_dir_all(&self.dir).map_err(|source| Error::CreateDir {
            path: self.dir.clone(),
            source,
        })?;
        let path = self.file_path(name);
        let contents = toml::to_string_pretty(config)?;
        fs::write(&path, contents).map_err(|source| Error::Write { path, source })
    }

    /// Remove a driver config file.
    pub fn remove(&self, name: &str) -> Result<()> {
        let path = self.file_path(name);
        fs::remove_file(&path).map_err(|source| Error::Remove { path, source })
    }

    /// Load all driver configs from the directory.
    pub fn load_all(&self) -> Result<Config> {
        let drivers = self
            .each_driver()?
            .map(|(_name, loaded)| loaded)
            .collect::<std::result::Result<Vec<_>, Error>>()?;
        Ok(Config { drivers })
    }

    /// Read a single driver spec by name.
    pub fn read_spec(&self, name: &str) -> Result<Driver> {
        let spec = self.read(name)?.into_spec(name.to_owned());
        spec.validate().map_err(|source| Error::InvalidDriverSpec {
            name: name.to_owned(),
            source,
        })?;
        Ok(spec)
    }

    /// Write (create or overwrite) a driver spec.
    pub fn write_spec(&self, spec: &Driver) -> Result<()> {
        spec.validate().map_err(|source| Error::InvalidDriverSpec {
            name: spec.name.clone(),
            source,
        })?;
        self.write(&spec.name, &DriverFileConfig::from_spec(spec))
    }

    /// Load all driver specs from the directory.
    pub fn load_all_specs(&self) -> Result<Vec<Driver>> {
        self.list()?
            .into_iter()
            .map(|name| self.read_spec(&name))
            .collect()
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
    use crate::domain::driver::{Driver, StorageSpec, TelemetrySpec};

    fn tmp_config_dir() -> (tempfile::TempDir, ConfigDir) {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let cd = ConfigDir::new(tmp.path().to_path_buf());
        (tmp, cd)
    }

    fn local_driver_spec(name: &str) -> Driver {
        Driver {
            name: name.to_owned(),
            path: PathBuf::from(format!("/mnt/{name}")),
            storage: StorageSpec::Local {
                root: PathBuf::from(format!("/data/{name}")),
            },
            telemetry: TelemetrySpec::default(),
        }
    }

    fn local_config() -> DriverFileConfig {
        DriverFileConfig {
            path: PathBuf::from("/mnt/local"),
            storage: StorageConfig::Local {
                root: PathBuf::from("/data"),
            },
            telemetry: TelemetryFileConfig::default(),
        }
    }

    fn onedrive_config() -> DriverFileConfig {
        DriverFileConfig {
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
        let cfg = DriverFileConfig {
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
    fn each_driver_yields_sorted_name_and_config() {
        let (_tmp, cd) = tmp_config_dir();
        cd.write("bravo", &local_config()).expect("write failed");
        cd.write("alpha", &onedrive_config()).expect("write failed");
        let entries: Vec<_> = cd.each_driver().expect("each_driver").collect();
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
    fn load_all_returns_all_drivers() {
        let (_tmp, cd) = tmp_config_dir();
        cd.write("a", &local_config()).expect("write failed");
        cd.write("b", &onedrive_config()).expect("write failed");
        let config = cd.load_all().expect("load_all failed");
        assert_eq!(config.drivers.len(), 2);
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
        let cfg = DriverFileConfig {
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
    fn driver_file_config_exposes_mount_path() {
        let cfg = local_config();
        assert_eq!(cfg.path, PathBuf::from("/mnt/local"));
    }

    #[test]
    fn write_spec_round_trips_driver_spec() {
        let (_tmp, cd) = tmp_config_dir();
        let spec = local_driver_spec("alpha");

        cd.write_spec(&spec).expect("write should work");

        let loaded = cd.read_spec("alpha").expect("read should work");
        assert_eq!(loaded, spec);
    }

    #[test]
    fn load_all_specs_preserves_driver_names() {
        let (_tmp, cd) = tmp_config_dir();
        cd.write_spec(&local_driver_spec("alpha"))
            .expect("write alpha");
        cd.write_spec(&local_driver_spec("beta"))
            .expect("write beta");

        let specs = cd.load_all_specs().expect("load should work");
        let names = specs.into_iter().map(|spec| spec.name).collect::<Vec<_>>();
        assert_eq!(names, vec!["alpha".to_owned(), "beta".to_owned()]);
    }
}
