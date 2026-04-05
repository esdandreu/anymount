use crate::DriverFileConfig;
use crate::domain::driver::DriverConfig;

#[derive(Debug, Clone)]
pub(crate) struct ProviderEntry {
    pub(crate) name: String,
    pub(crate) config: DriverFileConfig,
}

impl ProviderEntry {
    pub(crate) fn is_connected(&self) -> bool {
        crate::cli::provider_control::provider_daemon_ready(&self.name)
    }
}

pub(crate) fn provider_entry_from_spec(spec: DriverConfig) -> ProviderEntry {
    let name = spec.name.clone();
    ProviderEntry {
        name,
        config: DriverFileConfig {
            path: spec.path,
            storage: spec.storage.into(),
            telemetry: spec.telemetry.into(),
        },
    }
}
