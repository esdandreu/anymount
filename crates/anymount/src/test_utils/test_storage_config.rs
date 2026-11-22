use std::sync::Arc;

use crate::domain::{StorageConfig, StorageConnector};

use super::mocks::EmptyStorageConfig;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct TestStorageConfig {
    pub endpoint: Option<String>,
    pub bucket: String,
    pub username: String,
    pub password: String,
}

#[typetag::serde(name = "test")]
impl StorageConfig for TestStorageConfig {
    fn create_connector(&self, identity: String) -> Arc<dyn StorageConnector> {
        EmptyStorageConfig.create_connector(identity)
    }
}
