use std::collections::HashMap;

use crate::app::MountConfig;
use anymount::{Config, ConfigRepository, domain::Mount, mount};
use async_trait::async_trait;

#[async_trait(?Send)]
pub trait MountService {
    async fn list(&self) -> color_eyre::Result<Vec<MountConfig>>;
    async fn connect(&mut self, name: &str) -> color_eyre::Result<()>;
    async fn disconnect(&mut self, name: &str) -> color_eyre::Result<()>;
}

pub struct AnymountMountService {
    configurations: Vec<Config>,
    active_mounts: HashMap<String, Box<dyn Mount>>,
}

impl AnymountMountService {
    pub fn new(repository: impl ConfigRepository) -> Self {
        Self {
            configurations: repository.list().collect(),
            active_mounts: HashMap::new(),
        }
    }
}

#[async_trait(?Send)]
impl MountService for AnymountMountService {
    async fn list(&self) -> color_eyre::Result<Vec<MountConfig>> {
        let mut mounts = Vec::new();
        for config in &self.configurations {
            let name = config.mount.name.clone();
            let path = config.mount.path.clone();
            let storage_type = config.mount.storage.kind().to_owned();
            let is_connected = self
                .active_mounts
                .get(&name)
                .is_some_and(|mount| mount.is_connected());
            mounts.push(MountConfig {
                name,
                path,
                storage_type,
                is_connected,
            });
        }
        Ok(mounts)
    }

    async fn connect(&mut self, name: &str) -> color_eyre::Result<()> {
        if self.active_mounts.contains_key(name) {
            return Ok(());
        }
        let config = self
            .configurations
            .iter()
            .find(|config| config.mount.name == name)
            .ok_or_else(|| {
                color_eyre::eyre::eyre!("configuration '{name}' was not found")
            })?;
        let mount = mount(config).await?;
        mount.connect().await?;
        self.active_mounts.insert(name.to_owned(), mount);
        Ok(())
    }

    async fn disconnect(&mut self, name: &str) -> color_eyre::Result<()> {
        let mount = self.active_mounts.get(name).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "mount '{name}' was not connected by this TUI session"
            )
        })?;
        mount.disconnect().await?;
        self.active_mounts.remove(name);
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
