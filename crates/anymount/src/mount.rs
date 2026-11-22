use std::sync::Arc;

use crate::domain::{LoadedConfig, Mount};
use crate::drivers::{NoDefaultDriver, init_driver};

/// Mounts using the record's existing connector.
///
/// # Errors
///
/// Propagates driver selection and mount errors.
pub async fn mount(
    record: &LoadedConfig,
) -> Result<Box<dyn Mount>, MountError> {
    let driver = init_driver(record.config.driver.as_deref())?;
    driver
        .mount(&record.config.mount, Arc::clone(&record.connector))
        .await
        .map_err(MountError::from)
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

    use super::LoadedConfig;
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

        let record = LoadedConfig::new(config, "workspace".to_owned())
            .expect("open storage session");
        let mount = super::mount(&record).await.expect("mount should succeed");

        assert_eq!(mount.name(), "no-op");
    }
}
