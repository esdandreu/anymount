use super::callbacks::Callbacks;
use super::{Provider, ProviderConfiguration, Storage};
use cloud_filter::root::{
    Connection, HydrationType, PopulationType, SecurityId, Session, SyncRootId, SyncRootIdBuilder,
    SyncRootInfo,
};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

pub const ID_PREFIX: &'static str = "Anymount";

pub struct CloudFilterProvider<S: Storage> {
    path: PathBuf,
    session: Session,
    storage: S,
    id: SyncRootId,
    connection: OnceLock<Connection<Callbacks<S>>>,
}

impl<S: Storage> CloudFilterProvider<S> {
    pub fn new(config: &impl ProviderConfiguration, storage: S) -> Arc<Self> {
        // TODO This method can fail!
        let security_id = SecurityId::current_user().unwrap();
        Arc::new(Self {
            path: config.path(),
            storage,
            session: Session::new(),
            id: SyncRootIdBuilder::new(ID_PREFIX)
                .user_security_id(security_id)
                .build(),
            connection: OnceLock::new(),
        })
    }

    pub fn get_path(&self) -> &PathBuf {
        &self.path
    }

    pub fn get_display_name(&self) -> &str {
        self.path
            .file_name()
            .and_then(|os_str| os_str.to_str())
            .unwrap_or("Anymount")
    }

    pub fn create_sync_root_info(&self) -> windows::core::Result<SyncRootInfo> {
        SyncRootInfo::default()
            .with_display_name(self.get_display_name())
            .with_icon("%SystemRoot%\\system32\\charmap.exe,0")
            .with_version(env!("CARGO_PKG_VERSION"))
            .with_hydration_type(HydrationType::Full)
            .with_population_type(PopulationType::Full)
            .with_path(self.get_path())
    }
}

impl<S: Storage> Provider for Arc<CloudFilterProvider<S>> {
    fn kind(&self) -> &'static str {
        "CloudFilter"
    }
    fn path(&self) -> &PathBuf {
        &self.path
    }

    fn connect(&self) -> Result<(), String> {
        self.connect_arc()
    }
}

impl<S: Storage> CloudFilterProvider<S> {
    pub fn connect_arc(self: &Arc<Self>) -> Result<(), String> {
        // Check if already connected
        if self.connection.get().is_some() {
            return Err("Already connected".to_string());
        }

        let is_registered = self.id.is_registered().map_err(|e| e.to_string())?;
        if !is_registered {
            self.id
                .register(self.create_sync_root_info().map_err(|e| e.to_string())?)
                .map_err(|e| format!("Failed to register sync root: {}", e))?;
        }

        let wrapper = Callbacks(self.clone());
        let conn = self
            .session
            .connect(&self.path, wrapper)
            .map_err(|e| format!("Failed to connect to sync root: {}", e))?;

        self.connection
            .set(conn)
            .map_err(|_| "Already connected".to_string())?;
        Ok(())
    }
}
