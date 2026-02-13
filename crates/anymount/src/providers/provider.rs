use crate::storages::mock::MockStorage;
use std::path::PathBuf;
use std::result::Result;

pub trait ProvidersConfiguration {
    fn providers(&self) -> Vec<&impl ProviderConfiguration>;
}

pub trait ProviderConfiguration {
    fn path(&self) -> PathBuf;
}

pub trait Provider {
    fn kind(&self) -> &'static str;
    fn path(&self) -> &PathBuf;
}

#[cfg(target_os = "windows")]
pub fn connect_providers(
    config: &impl ProvidersConfiguration,
) -> Result<Vec<Box<dyn Provider>>, String> {
    use super::cloudfilter::{CloudFilterProvider, cleanup_registry};
    let mut providers: Vec<Box<dyn Provider>> = Vec::new();
    for provider in config.providers() {
        let storage = MockStorage::new();
        let provider = CloudFilterProvider::<MockStorage>::connect(provider, storage)?;
        providers.push(Box::new(provider));
    }
    // Cleanup any non-configured registered anymount sync root
    cleanup_registry(config)?;
    Ok(providers)
}

#[cfg(not(target_os = "windows"))]
pub fn connect_providers(
    _config: &impl ProvidersConfiguration,
) -> Result<Vec<Box<dyn Provider>>, String> {
    Err(String::from("Not supported on this platform"))
}
