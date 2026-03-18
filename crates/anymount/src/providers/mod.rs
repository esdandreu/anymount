#[cfg(target_os = "windows")]
pub mod cloudfilter;

#[cfg(target_os = "linux")]
pub mod libcloudprovider;

pub mod error;
pub mod provider;

pub use error::{Error, Result};
pub use provider::{
    Provider, ProviderConfiguration, ProvidersConfiguration, StorageConfig, connect_providers,
    connect_providers_with_telemetry,
};
