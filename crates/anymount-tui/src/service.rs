use std::collections::HashMap;

use anymount::{
    ConfigRepository, MountStatus, domain::Mount, find_and_mount, is_mounted,
};

use crate::app::MountConfig;

pub trait MountService {
    fn list(&self) -> color_eyre::Result<Vec<MountConfig>>;
    fn connect(&mut self, name: &str) -> color_eyre::Result<()>;
    fn disconnect(&mut self, name: &str) -> color_eyre::Result<()>;
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

impl<R> MountService for AnymountMountService<R>
where
    R: ConfigRepository,
{
    fn list(&self) -> color_eyre::Result<Vec<MountConfig>> {
        self.repository
            .list()
            .map(|config| {
                let name = config.name.clone();
                let path = config.path.clone();
                let storage_type = config.storage.kind().to_owned();
                let status = is_mounted(config)?;

                Ok(MountConfig {
                    name,
                    path,
                    storage_type,
                    is_connected: status == MountStatus::MountedAtExpectedRoot,
                })
            })
            .collect()
    }

    fn connect(&mut self, name: &str) -> color_eyre::Result<()> {
        let mount = find_and_mount(&self.repository, name)?;
        self.active_mounts.insert(name.to_owned(), mount);
        Ok(())
    }

    fn disconnect(&mut self, name: &str) -> color_eyre::Result<()> {
        let mount = self.active_mounts.get(name).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "mount '{name}' was not connected by this TUI session"
            )
        })?;
        mount.disconnect()?;
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

    #[test]
    fn lists_an_empty_repository() {
        let service = AnymountMountService::new(EmptyConfigRepository);

        assert!(service.list().expect("list should succeed").is_empty());
    }

    #[test]
    fn cannot_disconnect_mount_not_connected_by_session() {
        let mut service = AnymountMountService::new(EmptyConfigRepository);

        let error = service
            .disconnect("missing")
            .expect_err("disconnect should fail");

        assert_eq!(
            error.to_string(),
            "mount 'missing' was not connected by this TUI session"
        );
    }
}
