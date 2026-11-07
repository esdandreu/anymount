use crate::domain::{Config, Mount};
use crate::drivers::{NoDefaultDriver, init_driver};

pub async fn mount(config: Config) -> Result<Box<dyn Mount>, MountError> {
    let driver = init_driver(config.driver.as_deref())?;
    driver.mount(&config.mount).await.map_err(MountError::from)
}

#[derive(Debug, thiserror::Error)]
pub enum MountError {
    #[error(transparent)]
    DriverSelection(#[from] NoDefaultDriver),

    #[error("failed to mount: {0}")]
    CannotMount(#[from] crate::domain::MountError),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::domain::{Config, MountConfig};
    use crate::test_utils::mocks::{EmptyStorageConfig, NoOpDriverConfig};

    #[tokio::test]
    async fn mounts_with_an_explicit_driver() {
        let config = Config {
            driver: Some(Box::new(NoOpDriverConfig)),
            mount: MountConfig {
                name: "workspace".to_owned(),
                path: PathBuf::from("workspace"),
                storage: Box::new(EmptyStorageConfig),
            },
        };

        let mount = super::mount(config).await.expect("mount should succeed");

        assert_eq!(mount.name(), "no-op");
    }
}
