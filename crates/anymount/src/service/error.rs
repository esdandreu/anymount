#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid driver name: {name}")]
    InvalidDriverName { name: String },

    #[error("control message was not valid utf-8: {0}")]
    DecodeUtf8(#[from] std::str::Utf8Error),

    #[error("unknown control message: {value}")]
    UnknownControlMessage { value: String },

    #[error("service io error during {operation} for {driver_name}: {source}")]
    Io {
        operation: &'static str,
        driver_name: String,
        #[source]
        source: std::io::Error,
    },

    #[error("service receive failed: {0}")]
    Receive(#[from] std::sync::mpsc::RecvError),

    #[error("in-memory control transport was poisoned")]
    Poisoned,

    #[error("no in-memory server bound for driver {driver_name}")]
    NotBound { driver_name: String },

    #[error("no queued response available for driver {driver_name}")]
    NoQueuedResponse { driver_name: String },

    #[error("control transport not supported on this platform")]
    NotSupported,
}

pub type Result<T> = std::result::Result<T, Error>;
