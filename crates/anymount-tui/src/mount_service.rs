use std::collections::HashMap;

use crate::app::MountConfig;
use anymount::{ConfigRepository, domain::Mount, mount};
use async_trait::async_trait;

#[async_trait(?Send)]
pub trait MountService {
    async fn list(&self) -> color_eyre::Result<Vec<MountConfig>>;
    async fn connect(&mut self, name: &str) -> color_eyre::Result<()>;
    async fn disconnect(&mut self, name: &str) -> color_eyre::Result<()>;
}

pub struct AnymountMountService<R> {
    repository: R,
    active_mounts: HashMap<String, Box<dyn Mount>>,
}

impl<R> AnymountMountService<R> {
    pub fn new(repository: R) -> Self {
        Self {
            repository,
            active_mounts: HashMap::new(),
        }
    }
}

#[async_trait(?Send)]
impl<R> MountService for AnymountMountService<R>
where
    R: ConfigRepository,
{
    async fn list(&self) -> color_eyre::Result<Vec<MountConfig>> {
        let mut mounts = Vec::new();
        for config in self.repository.list() {
            let name = config.mount.name.clone();
            let path = config.mount.path.clone();
            let storage_type = config.mount.storage.kind().to_owned();
            let mount = mount(config).await?;
            mounts.push(MountConfig {
                name,
                path,
                storage_type,
                is_connected: mount.is_connected(),
            });
        }
        Ok(mounts)
    }

    async fn connect(&mut self, name: &str) -> color_eyre::Result<()> {
        if self.active_mounts.contains_key(name) {
            return Ok(());
        }
        let config = self.repository.find(name)?;
        let mount = mount(config).await?;
        if !mount.is_connected()
            && let Err(error) = mount.connect().await
        {
            if let Err(cleanup_error) = mount.unregister().await {
                return Err(color_eyre::eyre::eyre!(
                    "failed to connect mount: {error}; cleanup also failed: {cleanup_error}"
                ));
            }
            return Err(error.into());
        }
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
    use anymount::{Config, ConfigRepository};

    use super::{AnymountMountService, MountService};

    struct EmptyConfigRepository;

    impl ConfigRepository for EmptyConfigRepository {
        type Iter<'a> = std::iter::Empty<Config>;

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
