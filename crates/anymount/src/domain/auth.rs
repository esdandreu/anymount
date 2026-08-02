//! Defines storage authorization flows and errors.

use super::storage::StorageConfig;

/// Represents an authorization flow awaiting user completion.
pub trait StartedAuthorization {
    /// Returns instructions for completing authorization.
    fn message(&self) -> String;

    /// Returns a browser URI when the flow provides one.
    fn verification_uri(&self) -> Option<String> {
        None
    }

    /// Waits for authorization and returns the updated configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization cannot be completed.
    fn wait(
        self: Box<Self>,
    ) -> Result<Box<dyn StorageConfig>, AuthStorageError>;
}

/// Describes a storage authentication failure.
#[derive(Debug, thiserror::Error)]
pub enum AuthStorageError {
    #[error("{kind} storage does not implement authentication")]
    NotImplemented { kind: &'static str },

    #[error("failed to authenticate {kind} storage: {message}")]
    Failed { kind: &'static str, message: String },
}
