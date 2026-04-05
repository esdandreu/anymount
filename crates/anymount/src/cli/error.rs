#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Auth(#[from] crate::auth::Error),

    #[error(transparent)]
    Config(#[from] crate::config::Error),

    #[error(transparent)]
    Otlp(#[from] crate::telemetry::OtlpInitError),

    #[error(transparent)]
    Service(#[from] crate::service::Error),

    #[error(transparent)]
    Drivers(#[from] crate::drivers::Error),

    #[error("failed to serialize config: {source}")]
    SerializeConfig {
        #[source]
        source: toml::ser::Error,
    },

    #[error("invalid integer value {value}: {source}")]
    ParseInteger {
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },

    #[error("specify <NAME> or --all")]
    MissingConnectTarget,

    #[error("specify --name <NAME> or --all")]
    MissingDisconnectTarget,

    #[error("specify <NAME>, or use `temp` with add-like driver arguments")]
    MissingConnectSyncTarget,

    #[error("failed to install Ctrl-C handler: {source}")]
    InstallCtrlC {
        #[source]
        source: ctrlc::Error,
    },

    #[error("failed to resolve current executable: {source}")]
    ResolveCurrentExecutable {
        #[source]
        source: std::io::Error,
    },

    #[error("failed to spawn driver process for {driver_name}: {source}")]
    SpawnDriver {
        driver_name: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to wait for driver process {driver_name}: {source}")]
    WaitForDriver {
        driver_name: String,
        #[source]
        source: std::io::Error,
    },

    #[error("driver process {driver_name} exited before ready with status {status}")]
    DriverExitedBeforeReady { driver_name: String, status: String },

    #[error("driver process {driver_name} did not become ready")]
    DriverDidNotBecomeReady { driver_name: String },

    #[error("failed to connect drivers: {failures}")]
    ConnectFailures { failures: String },

    #[error("failed to disconnect drivers: {failures}")]
    DisconnectFailures { failures: String },

    #[error("{0}")]
    Prompt(String),

    #[error("{0}")]
    Validation(String),
}

pub type Result<T> = std::result::Result<T, Error>;
