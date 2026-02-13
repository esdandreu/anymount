use crate::storages::LocalStorage;
use std::path::PathBuf;
use std::result::Result;

pub enum StorageConfig {
    Local { root: PathBuf },
}

pub trait ProvidersConfiguration {
    fn providers(&self) -> Vec<&impl ProviderConfiguration>;
}

pub trait ProviderConfiguration {
    fn path(&self) -> PathBuf;
    fn storage_config(&self) -> StorageConfig;
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
    for provider_config in config.providers() {
        let provider = match provider_config.storage_config() {
            StorageConfig::Local { root } => {
                let storage = LocalStorage::new(root);
                CloudFilterProvider::<LocalStorage>::connect(provider_config, storage)?
            }
        };
        providers.push(Box::new(provider));
    }
    cleanup_registry(config)?;
    Ok(providers)
}

#[cfg(not(target_os = "windows"))]
pub fn connect_providers(
    _config: &impl ProvidersConfiguration,
) -> Result<Vec<Box<dyn Provider>>, String> {
    Err(String::from("Not supported on this platform"))
}
