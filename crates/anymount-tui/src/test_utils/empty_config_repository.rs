use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use anymount::domain::{
    ConfigRecordId, CredentialUpdate, SaveCredentialsError,
};
use anymount::{ConfigRepository, LoadedConfig};

#[derive(Clone, Default)]
pub struct EmptyConfigRepository {
    reads: Arc<AtomicUsize>,
}

impl EmptyConfigRepository {
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }
}

#[async_trait::async_trait(?Send)]
impl ConfigRepository for EmptyConfigRepository {
    fn list(&self) -> Box<dyn Iterator<Item = LoadedConfig> + '_> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        Box::new(std::iter::empty())
    }

    async fn save_credentials(
        &self,
        _record_id: &ConfigRecordId,
        _update: CredentialUpdate,
    ) -> Result<(), SaveCredentialsError> {
        Err(SaveCredentialsError)
    }
}
