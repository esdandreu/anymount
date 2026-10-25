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
