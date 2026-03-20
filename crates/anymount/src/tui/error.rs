#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Auth(#[from] crate::auth::Error),

    #[error(transparent)]
    Cli(#[from] crate::cli::Error),

    #[error(transparent)]
    Config(#[from] crate::config::Error),

    #[error("terminal operation {operation} failed: {source}")]
    Terminal {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid number for {key}: {value}")]
    InvalidNumber {
        key: String,
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },

    #[error("tui session error: {session}; terminal restore error: {restore}")]
    SessionRestore { session: String, restore: String },

    #[error("{0}")]
    Validation(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<crate::application::auth::Error> for Error {
    fn from(value: crate::application::auth::Error) -> Self {
        match value {
            crate::application::auth::Error::Auth(source) => Self::Auth(source),
        }
    }
}

impl From<crate::application::config::Error> for Error {
    fn from(value: crate::application::config::Error) -> Self {
        match value {
            crate::application::config::Error::Config(source) => Self::Config(source),
            crate::application::config::Error::DuplicateDriver { name } => {
                Self::Validation(format!("driver '{name}' already exists"))
            }
            crate::application::config::Error::InvalidStorageKey { key } => {
                Self::Validation(format!("'{key}' only applies to onedrive storage"))
            }
            crate::application::config::Error::UnknownKey { .. }
            | crate::application::config::Error::ParseInteger { .. } => {
                Self::Validation(value.to_string())
            }
        }
    }
}

impl From<crate::application::connect::Error> for Error {
    fn from(value: crate::application::connect::Error) -> Self {
        match value {
            crate::application::connect::Error::Config(source) => Self::Config(source),
            crate::application::connect::Error::Launch {
                driver_name,
                reason,
            } => Self::Validation(format!("failed to connect driver {driver_name}: {reason}")),
            crate::application::connect::Error::ConnectFailures { failures } => {
                Self::Validation(format!("failed to connect drivers: {failures}"))
            }
        }
    }
}
