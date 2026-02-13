use super::callbacks::Callbacks;
use super::{Provider, ProviderConfiguration, Storage};
use cloud_filter::root::{
    Connection, HydrationType, PopulationType, SecurityId, Session, SyncRootId, SyncRootIdBuilder,
    SyncRootInfo,
};
use std::path::{PathBuf, absolute};
use std::sync::Arc;
use tracing::info;

pub const ID_PREFIX: &'static str = "Anymount";

pub struct CloudFilterProvider<S: Storage> {
    path: PathBuf,
    #[allow(dead_code)]
    id: SyncRootId,
    #[allow(dead_code)]
    connection: Connection<Callbacks<S>>,
}

impl<S: Storage> CloudFilterProvider<S> {
    pub fn connect(config: &impl ProviderConfiguration, storage: S) -> Result<Arc<Self>, String> {
        let security_id = SecurityId::current_user().map_err(|e| e.to_string())?;
        let path = config.path();
        if !path.exists() {
            std::fs::create_dir(&path)
                .map_err(|e| format!("Failed to create mount path: {}", e))?;
        }
        info!("Mount path: {}", path.display());
        let path = absolute(path)
            .map_err(|e| format!("Mount path must exist and be accessible: {}", e))?;
        let name = path
            .file_name()
            .and_then(|os_str| os_str.to_str())
            .ok_or("Invalid path")?;
        let provider_name = ID_PREFIX.to_owned() + "|" + name;

        let id = SyncRootIdBuilder::new(provider_name)
            .user_security_id(security_id)
            .build();

        // Register if not already registered
        let is_registered = id.is_registered().map_err(|e| e.to_string())?;
        // TODO(GIA) Handle when registered to a different path
        if !is_registered {
            let sync_root_info = SyncRootInfo::default()
                .with_display_name(name)
                .with_icon("%SystemRoot%\\system32\\charmap.exe,0")
                .with_version(env!("CARGO_PKG_VERSION"))
                .with_hydration_type(HydrationType::Full)
                .with_population_type(PopulationType::Full)
                .with_path(&path)
                .map_err(|e| e.to_string())?;

            id.register(sync_root_info)
                .map_err(|e| format!("Failed to register sync root: {}", e))?;
            info!("Sync root registered: {}", name);
        }

        // Connect session
        let session = Session::new();
        let connection = session
            .connect(&path, Callbacks::new(path.clone(), storage))
            .map_err(|e| format!("Failed to connect to sync root: {}", e))?;

        Ok(Arc::new(Self {
            path,
            id,
            connection,
        }))
    }
}

impl<S: Storage> Provider for Arc<CloudFilterProvider<S>> {
    fn kind(&self) -> &'static str {
        "CloudFilter"
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}
