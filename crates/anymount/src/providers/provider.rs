use crate::Logger;
use crate::daemon::messages::DaemonMessage;
use crate::storages::{LocalStorage, OneDriveConfig};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::result::Result;
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StorageConfig {
    Local {
        root: PathBuf,
    },
    OneDrive {
        root: PathBuf,
        endpoint: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        access_token: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        refresh_token: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        client_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
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
    logger: &(impl Logger + 'static),
) -> Result<Vec<Box<dyn Provider>>, String> {
    connect_providers_with_telemetry(config, logger, None)
}

#[cfg(target_os = "windows")]
pub fn connect_providers_with_telemetry(
    config: &impl ProvidersConfiguration,
    logger: &(impl Logger + 'static),
    daemon_tx: Option<Sender<DaemonMessage>>,
) -> Result<Vec<Box<dyn Provider>>, String> {
    use super::cloudfilter::{CloudFilterProvider, cleanup_registry};
    let mut providers: Vec<Box<dyn Provider>> = Vec::new();
    for provider_config in config.providers() {
        match provider_config.storage_config() {
            StorageConfig::Local { root } => {
                let storage = LocalStorage::new(root);
                let provider = CloudFilterProvider::connect(
                    provider_config,
                    storage,
                    logger.clone(),
                    daemon_tx.clone(),
                )
                .map_err(|error| error.to_string())?;
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
                let provider = CloudFilterProvider::connect(
                    provider_config,
                    storage,
                    logger.clone(),
                    daemon_tx.clone(),
                )
                .map_err(|error| error.to_string())?;
                providers.push(Box::new(provider) as Box<dyn Provider>);
            }
        }
    }
    cleanup_registry(config, logger).map_err(|error| error.to_string())?;
    Ok(providers)
}

#[cfg(target_os = "linux")]
pub fn connect_providers(
    config: &impl ProvidersConfiguration,
    logger: &(impl Logger + 'static),
) -> Result<Vec<Box<dyn Provider>>, String> {
    connect_providers_with_telemetry(config, logger, None)
}

#[cfg(target_os = "linux")]
pub fn connect_providers_with_telemetry(
    config: &impl ProvidersConfiguration,
    logger: &(impl Logger + 'static),
    _daemon_tx: Option<Sender<DaemonMessage>>,
) -> Result<Vec<Box<dyn Provider>>, String> {
    use super::libcloudprovider::dbus::AccountExporter;
    use super::libcloudprovider::provider::{LibCloudProvider, export_on_dbus, mount_storage};
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let mut accounts: Vec<(std::path::PathBuf, AccountExporter)> = Vec::new();
    let mut sessions: Vec<(std::path::PathBuf, fuser::BackgroundSession)> = Vec::new();
    for provider_config in config.providers() {
        let path = provider_config.path();
        match provider_config.storage_config() {
            StorageConfig::Local { root } => {
                let storage = LocalStorage::new(root);
                let (mount_path, session) = mount_storage(path, storage, logger.clone())?;
                let name = mount_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Anymount")
                    .to_string();
                accounts.push((
                    mount_path.clone(),
                    AccountExporter {
                        name: name.clone(),
                        path: mount_path.display().to_string(),
                        icon: String::new(),
                        status: 0,
                        status_details: String::new(),
                    },
                ));
                sessions.push((mount_path, session));
            }
            StorageConfig::OneDrive {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            } => {
                let one_drive_config = OneDriveConfig {
                    root,
                    endpoint,
                    access_token,
                    refresh_token,
                    client_id,
                    token_expiry_buffer_secs,
                };
                let storage = one_drive_config.connect().map_err(|e| e.to_string())?;
                let (mount_path, session) = mount_storage(path, storage, logger.clone())?;
                let name = mount_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("OneDrive")
                    .to_string();
                accounts.push((
                    mount_path.clone(),
                    AccountExporter {
                        name,
                        path: mount_path.display().to_string(),
                        icon: String::new(),
                        status: 0,
                        status_details: String::new(),
                    },
                ));
                sessions.push((mount_path, session));
            }
        }
    }
    rt.block_on(export_on_dbus(&accounts, logger))?;
    let providers: Vec<Box<dyn Provider>> = sessions
        .into_iter()
        .map(|(path, session)| Box::new(LibCloudProvider::new(path, session)) as Box<dyn Provider>)
        .collect();
    Ok(providers)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn connect_providers(
    _config: &impl ProvidersConfiguration,
    _logger: &impl Logger,
) -> Result<Vec<Box<dyn Provider>>, String> {
    connect_providers_with_telemetry(_config, _logger, None)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn connect_providers_with_telemetry(
    _config: &impl ProvidersConfiguration,
    _logger: &impl Logger,
    _daemon_tx: Option<Sender<DaemonMessage>>,
) -> Result<Vec<Box<dyn Provider>>, String> {
    Err(String::from("Not supported on this platform"))
}
