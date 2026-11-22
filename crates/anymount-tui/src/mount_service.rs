use std::collections::HashMap;

use crate::app::MountConfig;
use anymount::{ConfigRepository, LoadedConfig, domain::Mount, mount};
use async_trait::async_trait;

#[async_trait(?Send)]
pub trait MountService {
    async fn list(&self) -> color_eyre::Result<Vec<MountConfig>>;
    async fn connect(&mut self, name: &str) -> color_eyre::Result<()>;
    async fn disconnect(&mut self, name: &str) -> color_eyre::Result<()>;
}

pub struct AnymountMountService {
    configurations: Vec<LoadedConfig>,
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
        Ok(self
            .configurations
            .iter()
            .map(|record| {
                let config = &record.config.mount;
                MountConfig {
                    name: config.name.clone(),
                    path: config.path.clone(),
                    storage_type: config.storage.kind().to_owned(),
                    is_connected: self
                        .active_mounts
                        .get(&config.name)
                        .is_some_and(|mount| mount.is_connected()),
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
            .ok_or_else(|| anymount::domain::ConfigNotFound {
                name: name.to_owned(),
            })?;
        let mount = mount(record).await?;
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
    use anymount::{ConfigRepository, LoadedConfig};

    use super::{AnymountMountService, MountService};

    struct EmptyConfigRepository;

    // private test util: Exercises an empty configuration repository.
    impl ConfigRepository for EmptyConfigRepository {
        type Iter<'a> = std::iter::Empty<LoadedConfig>;

        fn list(&self) -> Self::Iter<'_> {
            std::iter::empty()
        }
    }

    #[tokio::test]
    async fn lists_an_empty_repository() {
        let service = AnymountMountService::new(EmptyConfigRepository);

        assert!(
            service
                .list()
                .await
                .expect("list should succeed")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn cannot_disconnect_mount_not_connected_by_session() {
        let mut service = AnymountMountService::new(EmptyConfigRepository);

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
