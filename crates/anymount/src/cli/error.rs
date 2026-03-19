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
    Providers(#[from] crate::providers::Error),

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

    #[error("specify --name <NAME> or --all")]
    MissingConnectTarget,

    #[error("specify --name <NAME> or --all")]
    MissingDisconnectTarget,

    #[error("specify --name <NAME> or --path <PATH> with a storage subcommand")]
    MissingProvideTarget,

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

    #[error("failed to spawn provider process for {provider_name}: {source}")]
    SpawnProvider {
        provider_name: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to wait for provider process {provider_name}: {source}")]
    WaitForProvider {
        provider_name: String,
        #[source]
        source: std::io::Error,
    },

    #[error("provider process {provider_name} exited before ready with status {status}")]
    ProviderExitedBeforeReady {
        provider_name: String,
        status: String,
    },

    #[error("provider process {provider_name} did not become ready")]
    ProviderDidNotBecomeReady { provider_name: String },

    #[error("failed to connect providers: {failures}")]
    ConnectFailures { failures: String },

    #[error("failed to disconnect providers: {failures}")]
    DisconnectFailures { failures: String },

    #[error("{0}")]
    Prompt(String),

    #[error("{0}")]
    Validation(String),
}

pub type Result<T> = std::result::Result<T, Error>;
