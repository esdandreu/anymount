use std::collections::HashMap;
use std::sync::Arc;

use crate::app::MountConfig;
use anymount::connected_mount::{ConnectedMount, disconnect_all};
use anymount::{ConfigRepository, LoadedConfig};
use async_trait::async_trait;

#[async_trait(?Send)]
pub trait MountService {
    async fn list(&self) -> color_eyre::Result<Vec<MountConfig>>;
    async fn connect(&mut self, name: &str) -> color_eyre::Result<()>;
    async fn disconnect(&mut self, name: &str) -> color_eyre::Result<()>;
    async fn shutdown(&mut self) -> color_eyre::Result<()>;
}

pub struct AnymountMountService {
    repository: Arc<dyn ConfigRepository>,
    configurations: Vec<LoadedConfig>,
    active_mounts: HashMap<String, ConnectedMount>,
}

impl AnymountMountService {
    pub fn new(repository: impl ConfigRepository + 'static) -> Self {
        Self {
            configurations: repository.list().collect(),
            repository: Arc::new(repository),
            active_mounts: HashMap::new(),
        }
    }
}

#[async_trait(?Send)]
impl MountService for AnymountMountService {
    async fn list(&self) -> color_eyre::Result<Vec<MountConfig>> {
        Ok(self
            .configurations
            .iter()
            .map(|record| {
                let mount = &record.config.mount;
                MountConfig {
                    name: mount.name.clone(),
                    path: mount.path.clone(),
                    storage_type: mount.storage.kind().to_owned(),
                    is_connected: self
                        .active_mounts
                        .get(&mount.name)
                        .is_some_and(ConnectedMount::is_connected),
                }
            })
            .collect())
    }

    async fn connect(&mut self, name: &str) -> color_eyre::Result<()> {
        if self.active_mounts.contains_key(name) {
            return Ok(());
        }
        let record = self
            .configurations
            .iter()
            .find(|record| record.config.mount.name == name)
            .ok_or_else(|| {
                color_eyre::eyre::eyre!("configuration '{name}' was not found")
            })?;
        let mount =
            ConnectedMount::connect(record, Arc::clone(&self.repository))
                .await?;
        self.active_mounts.insert(name.to_owned(), mount);
        Ok(())
    }

    async fn disconnect(&mut self, name: &str) -> color_eyre::Result<()> {
        let mount = self.active_mounts.remove(name).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "mount '{name}' was not connected by this TUI session"
            )
        })?;
        mount.disconnect().await?;
        Ok(())
    }

    async fn shutdown(&mut self) -> color_eyre::Result<()> {
        disconnect_all(self.active_mounts.drain().map(|(_, mount)| mount))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::{AnymountMountService, MountService};

    use crate::test_utils::EmptyConfigRepository;

    #[tokio::test]
    async fn lists_an_empty_repository() {
        let service =
            AnymountMountService::new(EmptyConfigRepository::default());

        assert!(
            service
                .list()
                .await
                .expect("list should succeed")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn owns_configuration_records_without_reloading_for_each_operation() {
        let repository = EmptyConfigRepository::default();
        let mut service = AnymountMountService::new(repository.clone());
        service.list().await.expect("first list");
        service.list().await.expect("second list");
        assert!(service.connect("missing").await.is_err());
        assert_eq!(repository.reads(), 1);
    }

    #[tokio::test]
    async fn cannot_disconnect_mount_not_connected_by_session() {
        let mut service =
            AnymountMountService::new(EmptyConfigRepository::default());

        let error = service
            .disconnect("missing")
            .await
            .expect_err("disconnect should fail");

        assert_eq!(
            error.to_string(),
            "mount 'missing' was not connected by this TUI session"
        );
    }
}
