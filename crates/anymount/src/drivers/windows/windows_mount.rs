use super::callbacks::Callbacks;
use super::{Error, Result};
use crate::domain::{
    ConnectMountError, DisconnectMountError, Mount, Storage,
    UnregisterMountError,
};
use cloud_filter::root::{
    Connection, HydrationType, PopulationType, SecurityId,
    Session as CloudSession, SyncRootId, SyncRootIdBuilder, SyncRootInfo,
};
use parking_lot::Mutex;
use std::path::{Path, PathBuf, absolute};

const ID_PREFIX: &str = "Anymount";

fn sync_root_id(name: &str) -> Result<SyncRootId> {
    let security_id = SecurityId::current_user().map_err(|source| {
        Error::CloudFilterOperation {
            operation: "resolve current user security id",
            source,
        }
    })?;
    Ok(SyncRootIdBuilder::new(format!("{ID_PREFIX}|{name}"))
        .user_security_id(security_id)
        .build())
}

/// A registered Windows sync root with an optional live connection.
pub struct WindowsMount<S: Storage> {
    name: String,
    path: PathBuf,
    id: SyncRootId,
    connection: Mutex<Option<Connection<Callbacks<S>>>>,
}

impl<S: Storage + 'static> WindowsMount<S> {
    pub fn new(name: String, path: PathBuf, storage: S) -> Result<Self> {
        if !path.exists() {
            std::fs::create_dir_all(&path).map_err(|source| Error::Io {
                operation: "create mount path",
                path: path.clone(),
                source,
            })?;
        }
        tracing::info!(path = %path.display(), "mounting Windows sync root");
        let path = absolute(&path).map_err(|source| Error::Io {
            operation: "resolve mount path",
            path: path.clone(),
            source,
        })?;
        let id = sync_root_id(&name)?;

        let is_registered = id.is_registered().map_err(|source| {
            Error::CloudFilterOperation {
                operation: "check sync root registration",
                source,
            }
        })?;
        if !is_registered {
            let sync_root_info = SyncRootInfo::default()
                .with_display_name(&name)
                .with_icon("%SystemRoot%\\system32\\charmap.exe,0")
                .with_version(env!("CARGO_PKG_VERSION"))
                .with_hydration_type(HydrationType::Full)
                .with_population_type(PopulationType::Full)
                .with_path(&path)
                .map_err(|source| Error::CloudFilterOperation {
                    operation: "build sync root info",
                    source,
                })?;
            id.register(sync_root_info).map_err(|source| {
                Error::CloudFilterOperation {
                    operation: "register sync root",
                    source,
                }
            })?;
        }

        let connection = CloudSession::new()
            .connect(&path, Callbacks::new(path.clone(), storage))
            .map_err(|source| Error::CloudFilterOperation {
                operation: "connect to sync root",
                source,
            })?;

        Ok(Self {
            name,
            path,
            id,
            connection: Mutex::new(Some(connection)),
        })
    }
}

impl<S: Storage + 'static> Mount for WindowsMount<S> {
    fn name(&self) -> &str {
        &self.name
    }

    fn root(&self) -> &Path {
        &self.path
    }

    fn is_connected(&self) -> bool {
        self.connection.lock().is_some()
    }

    fn connect(&self) -> std::result::Result<(), ConnectMountError> {
        if self.is_connected() {
            Ok(())
        } else {
            Err(ConnectMountError {
                path: self.path.clone(),
                message:
                    "a disconnected CloudFilter mount cannot be reconnected"
                        .to_owned(),
            })
        }
    }

    fn disconnect(&self) -> std::result::Result<(), DisconnectMountError> {
        self.connection.lock().take();
        Ok(())
    }

    fn unregister(&self) -> std::result::Result<(), UnregisterMountError> {
        self.connection.lock().take();
        self.id.unregister().map_err(|source| UnregisterMountError {
            path: self.path.clone(),
            message: source.to_string(),
        })
    }
}
