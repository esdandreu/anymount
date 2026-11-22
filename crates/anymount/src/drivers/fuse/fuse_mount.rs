use crate::domain::{
    ConnectMountError, DisconnectMountError, Mount, UnregisterMountError,
};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct FuseMount {
    name: String,
    path: PathBuf,
    session: Arc<Mutex<Option<fuser::BackgroundSession>>>,
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
            session: Arc::new(Mutex::new(Some(session))),
        }
    }
}

impl Drop for FuseMount {
    fn drop(&mut self) {
        if let Some(session) = self.session.lock().take()
            && let Err(error) = session.umount_and_join()
        {
            tracing::warn!(%error, "could not shut down FUSE session");
        }
    }
}

#[async_trait(?Send)]
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

    async fn connect(&self) -> Result<(), ConnectMountError> {
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

    async fn disconnect(&self) -> Result<(), DisconnectMountError> {
        let session = Arc::clone(&self.session);
        let disconnect_error = |message| DisconnectMountError {
            path: self.path.clone(),
            message,
        };
        tokio::task::spawn_blocking(move || {
            // Drop must wait for this join if the disconnect is canceled.
            let mut guard = session.lock();
            match guard.take() {
                Some(session) => session.umount_and_join(),
                None => Ok(()),
            }
        })
        .await
        .map_err(|error| disconnect_error(error.to_string()))?
        .map_err(|error| disconnect_error(error.to_string()))?;
        Ok(())
    }

    async fn unregister(&self) -> Result<(), UnregisterMountError> {
        self.disconnect()
            .await
            .map_err(|error| UnregisterMountError {
                path: error.path,
                message: error.message,
            })?;
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
