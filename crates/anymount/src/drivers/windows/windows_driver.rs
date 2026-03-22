use super::{Error, Result};
use crate::Logger;
use crate::drivers::Session;
use crate::service::control::messages::ServiceMessage;
use crate::storages::Storage;
use cloud_filter::root::{
    Connection, HydrationType, PopulationType, SecurityId, Session as CloudSession, SyncRootId,
    SyncRootIdBuilder, SyncRootInfo,
};
use std::path::{PathBuf, absolute};
use std::sync::mpsc::Sender;

pub const ID_PREFIX: &'static str = "Anymount";

pub struct WindowsSession<S, L>
where
    S: Storage,
    L: Logger,
{
    path: PathBuf,
    #[allow(dead_code)]
    id: SyncRootId,
    #[allow(dead_code)]
    connection: Option<Connection<super::Callbacks<S, L>>>,
}

impl<S, L> WindowsSession<S, L>
where
    S: Storage,
    L: Logger,
{
    pub fn connect(
        path: PathBuf,
        storage: S,
        logger: L,
        service_tx: Option<Sender<ServiceMessage>>,
    ) -> Result<Box<dyn Session>> {
        let security_id =
            SecurityId::current_user().map_err(|source| Error::CloudFilterOperation {
                operation: "resolve current user security id",
                source,
            })?;
        if !path.exists() {
            std::fs::create_dir(&path).map_err(|source| Error::Io {
                operation: "create mount path",
                path: path.clone(),
                source,
            })?;
        }
        logger.info(format!("Mount path: {}", path.display()));
        let path = absolute(&path).map_err(|source| Error::Io {
            operation: "resolve mount path",
            path: path.clone(),
            source,
        })?;
        let name = path
            .file_name()
            .and_then(|os_str| os_str.to_str())
            .ok_or_else(|| Error::InvalidPath { path: path.clone() })?;
        let driver_name = ID_PREFIX.to_owned() + "|" + name;

        let id = SyncRootIdBuilder::new(driver_name)
            .user_security_id(security_id)
            .build();

        let is_registered = id
            .is_registered()
            .map_err(|source| Error::CloudFilterOperation {
                operation: "check sync root registration",
                source,
            })?;
        if !is_registered {
            let sync_root_info = SyncRootInfo::default()
                .with_display_name(name)
                .with_icon("%SystemRoot%\\system32\\charmap.exe,0")
                .with_version(env!("CARGO_PKG_VERSION"))
                .with_hydration_type(HydrationType::Full)
                .with_population_type(PopulationType::Full)
                .with_path(&path)
                .map_err(|source| Error::CloudFilterOperation {
                    operation: "build sync root info",
                    source,
                })?;

            id.register(sync_root_info)
                .map_err(|source| Error::CloudFilterOperation {
                    operation: "register sync root",
                    source,
                })?;
            logger.info(format!("Sync root registered: {}", name));
        }

        let session = CloudSession::new();
        let connection = session
            .connect(
                &path,
                super::Callbacks::new(path.clone(), storage, logger, service_tx),
            )
            .map_err(|source| Error::CloudFilterOperation {
                operation: "connect to sync root",
                source,
            })?;

        Ok(Box::new(Self {
            path,
            id,
            connection: Some(connection),
        }))
    }
}

impl<S, L> Session for WindowsSession<S, L>
where
    S: Storage,
    L: Logger,
{
    fn kind(&self) -> &'static str {
        "CloudFilter"
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}
