use super::Result;
use crate::domain::provider::{ProviderSpec, StorageSpec};
use crate::service::control::messages::ServiceMessage;
use crate::storages::{LocalStorage, OneDriveConfig};
use crate::Logger;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

pub trait Provider {
    fn kind(&self) -> &'static str;
    fn path(&self) -> &PathBuf;
}

#[cfg(target_os = "windows")]
pub fn connect_providers(
    specs: &[ProviderSpec],
    logger: &(impl Logger + 'static),
) -> Result<Vec<Box<dyn Provider>>> {
    connect_providers_with_telemetry(specs, logger, None)
}

#[cfg(target_os = "windows")]
pub fn connect_providers_with_telemetry(
    specs: &[ProviderSpec],
    logger: &(impl Logger + 'static),
    service_tx: Option<Sender<ServiceMessage>>,
) -> Result<Vec<Box<dyn Provider>>> {
    use super::cloudfilter::{cleanup_registry, CloudFilterProvider};
    let mut providers: Vec<Box<dyn Provider>> = Vec::new();
    for spec in specs {
        match &spec.storage {
            StorageSpec::Local { root } => {
                let storage = LocalStorage::new(root.clone());
                let provider = CloudFilterProvider::connect(
                    spec.path.clone(),
                    storage,
                    logger.clone(),
                    service_tx.clone(),
                )?;
                providers.push(Box::new(provider) as Box<dyn Provider>);
            }
            StorageSpec::OneDrive {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            } => {
                let config = OneDriveConfig {
                    root: root.clone(),
                    endpoint: endpoint.clone(),
                    access_token: access_token.clone(),
                    refresh_token: refresh_token.clone(),
                    client_id: client_id.clone(),
                    token_expiry_buffer_secs: *token_expiry_buffer_secs,
                };
                let storage = config.connect()?;
                let provider = CloudFilterProvider::connect(
                    spec.path.clone(),
                    storage,
                    logger.clone(),
                    service_tx.clone(),
                )?;
                providers.push(Box::new(provider) as Box<dyn Provider>);
            }
        }
    }
    cleanup_registry(specs, logger)?;
    Ok(providers)
}

#[cfg(target_os = "linux")]
pub fn connect_providers(
    specs: &[ProviderSpec],
    logger: &(impl Logger + 'static),
) -> Result<Vec<Box<dyn Provider>>> {
    connect_providers_with_telemetry(specs, logger, None)
}

#[cfg(target_os = "linux")]
pub fn connect_providers_with_telemetry(
    specs: &[ProviderSpec],
    logger: &(impl Logger + 'static),
    _service_tx: Option<Sender<ServiceMessage>>,
) -> Result<Vec<Box<dyn Provider>>> {
    use super::libcloudprovider::dbus::AccountExporter;
    use super::libcloudprovider::provider::{
        export_on_dbus, mount_storage, new_runtime, LibCloudProvider,
    };
    let rt = new_runtime()?;
    let mut accounts: Vec<(std::path::PathBuf, AccountExporter)> = Vec::new();
    let mut sessions: Vec<(std::path::PathBuf, fuser::BackgroundSession)> = Vec::new();
    for spec in specs {
        let path = spec.path.clone();
        match &spec.storage {
            StorageSpec::Local { root } => {
                let storage = LocalStorage::new(root.clone());
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
            StorageSpec::OneDrive {
                root,
                endpoint,
                access_token,
                refresh_token,
                client_id,
                token_expiry_buffer_secs,
            } => {
                let one_drive_config = OneDriveConfig {
                    root: root.clone(),
                    endpoint: endpoint.clone(),
                    access_token: access_token.clone(),
                    refresh_token: refresh_token.clone(),
                    client_id: client_id.clone(),
                    token_expiry_buffer_secs: *token_expiry_buffer_secs,
                };
                let storage = one_drive_config.connect()?;
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
    _specs: &[ProviderSpec],
    _logger: &impl Logger,
) -> Result<Vec<Box<dyn Provider>>> {
    connect_providers_with_telemetry(_specs, _logger, None)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn connect_providers_with_telemetry(
    _specs: &[ProviderSpec],
    _logger: &impl Logger,
    _service_tx: Option<Sender<ServiceMessage>>,
) -> Result<Vec<Box<dyn Provider>>> {
    Err(super::Error::NotSupported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::provider::{ProviderSpec, StorageSpec, TelemetrySpec};
    use crate::NoOpLogger;

    #[test]
    fn storage_label_comes_from_domain_storage_spec() {
        let local = StorageSpec::Local {
            root: PathBuf::from("/data"),
        };
        assert_eq!(local.label(), "local");
        let onedrive = StorageSpec::OneDrive {
            root: PathBuf::from("/"),
            endpoint: "https://graph.microsoft.com/v1.0".to_owned(),
            access_token: None,
            refresh_token: None,
            client_id: None,
            token_expiry_buffer_secs: None,
        };
        assert_eq!(onedrive.label(), "onedrive");
    }

    fn local_provider_spec(name: &str) -> ProviderSpec {
        ProviderSpec {
            name: name.to_owned(),
            path: PathBuf::from(format!("/mnt/{name}")),
            storage: StorageSpec::Local {
                root: PathBuf::from(format!("/data/{name}")),
            },
            telemetry: TelemetrySpec::default(),
        }
    }

    #[test]
    fn connect_providers_accepts_resolved_specs() {
        let spec = local_provider_spec("demo");
        let result = connect_providers(&[spec], &NoOpLogger::default());
        assert!(!matches!(result, Err(crate::providers::Error::Storage(_))));
    }
}
