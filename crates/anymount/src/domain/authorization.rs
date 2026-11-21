//! Host-owned OAuth interaction and independent credential persistence.
//!
//! Implementations retain provider-owned configurations, not universal tokens.
//! A host owns one live session per saved record and prevents concurrent owners
//! across processes or tabs. UI and saving never run as storage side effects.

use super::StorageConfig;
use crate::drivers::oauth::RuntimeBounds;
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

/// Retains a credential-bearing configuration until its host saves it.
pub struct CredentialUpdate {
    pub generation: u64,
    /// Saved provider data for this generation.
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

/// Reports persistence independently from usable in-memory credentials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveStatus {
    Clean,
    Pending { generation: u64 },
    Saved { generation: u64 },
    Failed { generation: u64 },
}

/// Indicates a host configuration save failure, never an authorization failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("host could not save the latest credential configuration")]
pub struct HostSaveError;

/// Persists a session's configuration without participating in storage requests.
#[async_trait(?Send)]
pub trait ConfigurationPersistence: RuntimeBounds {
    /// Saves this snapshot while preserving unrelated configuration fields.
    ///
    /// # Errors
    /// Returns a separate host-save failure. Implementations must finish their
    /// write before returning; the session serializes calls for this record.
    async fn save(
        &self,
        update: &CredentialUpdate,
    ) -> Result<(), HostSaveError>;
}

/// Provides OAuth lifecycle operations independently of storage and user interfaces.
#[async_trait(?Send)]
pub trait Authorization {
    /// Starts PKCE navigation; retain its continuation before leaving the page.
    async fn begin_redirect(
        &self,
        redirect_uri: &str,
    ) -> Result<RedirectAuthorization, AuthorizationError>;
    /// Restores a bounded continuation after full-page navigation.
    fn resume_redirect(
        &self,
        continuation: &str,
    ) -> Result<(), AuthorizationError>;
    /// Consumes a matching callback once and installs the complete new grant.
    async fn complete_redirect(
        &self,
        callback: &str,
    ) -> Result<(), AuthorizationError>;
    /// Requests provider-supplied device instructions without opening user interfaces.
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
    /// Returns the newest unsaved configuration without consuming it.
    fn pending_update(&self) -> Option<CredentialUpdate>;
    /// Reports the independent persistence outcome.
    fn save_status(&self) -> SaveStatus;
    /// Waits for a newer credential generation without polling.
    async fn wait_for_update(&self, after_generation: u64) -> u64;
    /// Serializes saves and catches up to the newest received generation.
    async fn save_pending(
        &self,
        store: &dyn ConfigurationPersistence,
    ) -> Result<(), HostSaveError>;
}
