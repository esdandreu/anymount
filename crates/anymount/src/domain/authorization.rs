//! Host-owned OAuth interaction and versioned credential updates.
//!
//! A host retains one connector per saved record and prevents concurrent
//! owners across processes or tabs. Storage requests neither open UI nor save.

use super::StorageConfig;
use async_trait::async_trait;
use std::fmt;

/// Classifies actionable authorization outcomes without exposing provider data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AuthorizationError {
    #[error("authorization service temporarily unavailable")]
    Temporary,
    #[error("authorization deployment or configuration is unsupported")]
    Setup,
    #[error("sign-in is required")]
    SignInRequired,
    #[error("resource permission denied")]
    PermissionDenied,
    #[error("authorization was declined")]
    Declined,
    #[error("authorization attempt expired")]
    Expired,
    #[error("authorization attempt cancelled")]
    Cancelled,
    #[error("authorization callback is invalid or already consumed")]
    InvalidCallback,
}

/// Describes navigation and the sensitive, short-lived continuation to retain.
pub struct RedirectAuthorization {
    pub authorization_url: String,
    pub continuation: String,
    pub expires_at: u64,
}

impl fmt::Debug for RedirectAuthorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedirectAuthorization")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// Contains provider-supplied instructions intended only for the signing-in user.
pub struct DeviceAuthorization {
    pub verification_uri: String,
    pub user_code: String,
    pub verification_uri_complete: Option<String>,
    pub expires_at: u64,
}

impl fmt::Debug for DeviceAuthorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceAuthorization")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

pub struct CredentialUpdate {
    pub generation: u64,
    /// Credential-bearing snapshot, not the original loaded configuration.
    pub config: Box<dyn StorageConfig>,
    /// Provider tag used only to reject updates to a different storage type.
    pub storage_type: &'static str,
    /// Nonempty object paths relative to `storage`, excluding the `type` tag.
    ///
    /// A missing or null value at a listed path removes that field. Unlisted
    /// fields, including siblings of a listed path, are not modified.
    pub fields: &'static [&'static [&'static str]],
}

impl fmt::Debug for CredentialUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialUpdate")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

#[async_trait(?Send)]
pub trait Authorization {
    /// Starts PKCE navigation; retain its continuation before leaving the page.
    async fn begin_redirect(
        &self,
        redirect_uri: &str,
    ) -> Result<RedirectAuthorization, AuthorizationError>;
    fn resume_redirect(
        &self,
        continuation: &str,
    ) -> Result<(), AuthorizationError>;
    /// Consumes a matching callback once and installs the complete new grant.
    async fn complete_redirect(
        &self,
        callback: &str,
    ) -> Result<(), AuthorizationError>;
    async fn begin_device(
        &self,
    ) -> Result<DeviceAuthorization, AuthorizationError>;
    /// Polls asynchronously until success, decline, expiry, or explicit cancellation.
    async fn poll_device(&self) -> Result<(), AuthorizationError>;
    /// Stops accepting the active attempt without revoking the existing grant.
    fn cancel(&self);
    /// Allows the next operation to retry a completed temporary renewal failure.
    ///
    /// Returns false for active exchanges and unusable grants. The host must
    /// choose this explicitly: a lost response may have rotated the grant.
    fn retry_renewal(&self) -> bool;
    /// Returns the latest changed credentials after the supplied generation.
    fn credential_snapshot(
        &self,
        after_generation: u64,
    ) -> Option<CredentialUpdate>;
    /// Waits for a newer credential generation without polling.
    async fn wait_for_update(&self, after_generation: u64) -> u64;
}
