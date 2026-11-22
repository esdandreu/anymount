use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use super::mocks::NoOpStorage;
use crate::domain::{
    Authorization, AuthorizationError, ConnectStorageError, CredentialUpdate,
    DeviceAuthorization, RedirectAuthorization, Storage, StorageConfig,
    StorageConnector,
};

#[derive(Clone, Deserialize, Serialize)]
pub struct FixtureStorageConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    endpoint: Option<String>,
    bucket: String,
    username: String,
    password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    retired_code: Option<String>,
}

#[typetag::serde(name = "test")]
impl StorageConfig for FixtureStorageConfig {
    fn create_connector(
        &self,
        _identity: String,
    ) -> Result<Arc<dyn StorageConnector>, AuthorizationError> {
        Ok(Arc::new(FixtureStorageConnector {
            saved: self.clone(),
            generation: watch::channel(0).0,
        }))
    }
}

struct FixtureStorageConnector {
    saved: FixtureStorageConfig,
    generation: watch::Sender<u64>,
}

#[async_trait(?Send)]
impl StorageConnector for FixtureStorageConnector {
    fn authorization(&self) -> Option<&dyn Authorization> {
        Some(self)
    }

    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        if self.saved.password.is_empty() && *self.generation.borrow() == 0 {
            return Err(ConnectStorageError::NotAuthenticated { kind: "test" });
        }
        Ok(Box::new(NoOpStorage))
    }
}

#[async_trait(?Send)]
impl Authorization for FixtureStorageConnector {
    async fn begin_redirect(
        &self,
        _redirect_uri: &str,
    ) -> Result<RedirectAuthorization, AuthorizationError> {
        Err(AuthorizationError::Setup)
    }

    fn resume_redirect(
        &self,
        _continuation: &str,
    ) -> Result<(), AuthorizationError> {
        Err(AuthorizationError::Setup)
    }

    async fn complete_redirect(
        &self,
        _callback: &str,
    ) -> Result<(), AuthorizationError> {
        Err(AuthorizationError::Setup)
    }

    async fn begin_device(
        &self,
    ) -> Result<DeviceAuthorization, AuthorizationError> {
        Ok(DeviceAuthorization {
            verification_uri: "https://fixture.example/device".to_owned(),
            user_code: "test-code".to_owned(),
            verification_uri_complete: None,
            expires_at: 2_000_000_000,
        })
    }

    async fn poll_device(&self) -> Result<(), AuthorizationError> {
        self.generation.send_modify(|generation| *generation += 1);
        Ok(())
    }

    fn cancel(&self) {}

    fn retry_renewal(&self) -> bool {
        false
    }

    fn credential_snapshot(
        &self,
        after_generation: u64,
    ) -> Option<CredentialUpdate> {
        let generation = *self.generation.borrow();
        if generation <= after_generation {
            return None;
        }
        let mut config = self.saved.clone();
        config.password = "rotated-password".to_owned();
        config.retired_code = None;
        Some(CredentialUpdate {
            generation,
            config: Box::new(config),
            storage_type: "test",
            fields: &[&["password"], &["retired_code"]],
        })
    }

    async fn wait_for_update(&self, after_generation: u64) -> u64 {
        let mut receiver = self.generation.subscribe();
        let generation = *receiver
            .wait_for(|current| *current > after_generation)
            .await
            .expect("fixture session retains sender");
        generation
    }
}
