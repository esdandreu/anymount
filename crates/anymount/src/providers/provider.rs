use crate::storages::{LocalStorage, OneDriveConfig, OneDriveStorage};
use std::path::PathBuf;
use std::result::Result;

pub enum StorageConfig {
    Local { root: PathBuf },
    OneDrive {
        root: PathBuf,
        endpoint: String,
        access_token: Option<String>,
        refresh_token: Option<String>,
        client_id: Option<String>,
        token_expiry_buffer_secs: Option<u64>,
    },
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
    use super::cloudfilter::{cleanup_registry, CloudFilterProvider};
    let mut providers: Vec<Box<dyn Provider>> = Vec::new();
    for provider_config in config.providers() {
        match provider_config.storage_config() {
            StorageConfig::Local { root } => {
                let storage = LocalStorage::new(root);
                let provider =
                    CloudFilterProvider::<LocalStorage>::connect(provider_config, storage)?;
                providers.push(Box::new(provider) as Box<dyn Provider>);
            }
            StorageConfig::OneDrive {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            } => {
                let config = OneDriveConfig {
                    root,
                    endpoint,
                    access_token,
                    refresh_token,
                    client_id,
                    token_expiry_buffer_secs,
                };
                let storage = config.connect().map_err(|e| e.to_string())?;
                let provider =
                    CloudFilterProvider::<OneDriveStorage>::connect(provider_config, storage)?;
                providers.push(Box::new(provider) as Box<dyn Provider>);
            }
        }
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
