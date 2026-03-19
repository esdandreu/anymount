#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid provider name: {name}")]
    InvalidProviderName { name: String },

    #[error("control message was not valid utf-8: {0}")]
    DecodeUtf8(#[from] std::str::Utf8Error),

    #[error("unknown control message: {value}")]
    UnknownControlMessage { value: String },

    #[error("service io error during {operation} for {provider_name}: {source}")]
    Io {
        operation: &'static str,
        provider_name: String,
        #[source]
        source: std::io::Error,
    },

    #[error("service receive failed: {0}")]
    Receive(#[from] std::sync::mpsc::RecvError),

    #[error("in-memory control transport was poisoned")]
    Poisoned,

    #[error("no in-memory server bound for provider {provider_name}")]
    NotBound { provider_name: String },

    #[error("no queued response available for provider {provider_name}")]
    NoQueuedResponse { provider_name: String },

    #[error("control transport not supported on this platform")]
    NotSupported,
}

pub type Result<T> = std::result::Result<T, Error>;
