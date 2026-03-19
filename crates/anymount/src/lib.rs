pub mod auth;
pub mod cli;
pub mod config;
pub mod daemon;
#[deprecated(note = "use module-specific errors instead")]
pub mod error;
pub mod logger;
pub mod providers;
pub mod storages;
pub mod telemetry;
pub mod tui;

pub use cli::commands::connect::{DefaultProviderProcessSupervisor, ProviderProcessSupervisor};
pub use config::{Config, ConfigDir, ProviderFileConfig, TelemetryFileConfig};
pub use logger::{Logger, NoOpLogger, TracingLogger};
pub use providers::{
    Provider, ProviderConfiguration, ProvidersConfiguration, StorageConfig, connect_providers,
    connect_providers_with_telemetry,
};
pub use storages::Storage;
