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
