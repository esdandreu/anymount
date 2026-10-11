use crate::domain::{
    ConnectMountError, DisconnectMountError, Mount, UnregisterMountError,
};
use parking_lot::Mutex;
use std::path::{Path, PathBuf};

pub struct FuseMount {
    name: String,
    path: PathBuf,
    session: Mutex<Option<fuser::BackgroundSession>>,
}

impl FuseMount {
    pub fn new(
        name: String,
        path: PathBuf,
        session: fuser::BackgroundSession,
    ) -> Self {
        Self {
            name,
            path,
            session: Mutex::new(Some(session)),
        }
    }
}

impl Mount for FuseMount {
    fn name(&self) -> &str {
        &self.name
    }

    fn root(&self) -> &Path {
        &self.path
    }

    fn is_connected(&self) -> bool {
        self.session.lock().is_some()
    }

    fn connect(&self) -> Result<(), ConnectMountError> {
        if self.is_connected() {
            Ok(())
        } else {
            Err(ConnectMountError {
                path: self.path.clone(),
                message: "an unmounted FUSE session cannot be reconnected"
                    .to_owned(),
            })
        }
    }

    fn disconnect(&self) -> Result<(), DisconnectMountError> {
        self.session.lock().take();
        Ok(())
    }

    fn unregister(&self) -> Result<(), UnregisterMountError> {
        self.session.lock().take();
        std::fs::remove_dir(&self.path).map_err(|source| {
            UnregisterMountError {
                path: self.path.clone(),
                message: source.to_string(),
            }
        })?;
        Ok(())
    }
}

impl std::fmt::Debug for FuseMount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FuseMount")
            .field("name", &self.name)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}
