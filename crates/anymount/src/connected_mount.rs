//! Hosts native mounts and persists changes to their shared credentials.

use crate::domain::{
    Authorization, ConfigRecordId, ConfigRepository, ConnectMountError,
    DisconnectMountError, LoadedConfig, Mount, StorageConnector,
    UnregisterMountError,
};
use std::{io, sync::Arc, thread};
use tokio::sync::oneshot;

/// A native connection with its host-owned credential watcher.
pub struct ConnectedMount {
    // Stop native requests before the watcher's final snapshot, even on drop.
    mount: Box<dyn Mount>,
    watch: Option<CredentialWatch>,
}

impl ConnectedMount {
    /// Connects a configured mount and watches its authorization for changes.
    ///
    /// # Errors
    ///
    /// Returns watcher startup, mount registration, or connection errors.
    pub async fn connect(
        record: &LoadedConfig,
        repository: Arc<dyn ConfigRepository>,
    ) -> Result<Self, ConnectError> {
        let watch = match &record.record_id {
            Some(id) if record.connector.authorization().is_some() => {
                Some(CredentialWatch::start(
                    Arc::clone(&record.connector),
                    repository,
                    Arc::clone(id),
                )?)
            }
            _ => None,
        };
        let mount = crate::mount(record).await?;
        let connection = Self { mount, watch };
        connection.mount.connect().await?;
        Ok(connection)
    }

    /// Reports whether the driver is still connected.
    pub fn is_connected(&self) -> bool {
        self.mount.is_connected()
    }

    /// Stops native requests and drains pending credential saves.
    ///
    /// # Errors
    ///
    /// Returns the driver's disconnect error.
    pub async fn disconnect(self) -> Result<(), DisconnectMountError> {
        let result = self.mount.disconnect().await;
        self.finish().await;
        result
    }

    /// Removes the registration and drains pending credential saves.
    ///
    /// # Errors
    ///
    /// Returns the driver's unregister error.
    pub async fn unregister(self) -> Result<(), UnregisterMountError> {
        let result = self.mount.unregister().await;
        self.finish().await;
        result
    }

    async fn finish(self) {
        drop(self.mount);
        if let Some(watch) = self.watch {
            watch.finish().await;
        }
    }
}

/// Disconnects every mount, even if an earlier disconnect fails.
///
/// # Errors
///
/// Returns the first disconnect error.
pub async fn disconnect_all(
    mounts: impl IntoIterator<Item = ConnectedMount>,
) -> Result<(), DisconnectMountError> {
    let mut result = Ok(());
    for mount in mounts {
        if let Err(error) = mount.disconnect().await {
            tracing::warn!(%error, "could not disconnect mount");
            result = result.and(Err(error));
        }
    }
    result
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("could not start credential watcher: {0}")]
    Watch(#[from] io::Error),
    #[error(transparent)]
    Mount(#[from] crate::MountError),
    #[error(transparent)]
    Connect(#[from] ConnectMountError),
}

struct CredentialWatch {
    stop: Option<oneshot::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
    finished: oneshot::Receiver<()>,
}

impl CredentialWatch {
    fn start(
        connector: Arc<dyn StorageConnector>,
        repository: Arc<dyn ConfigRepository>,
        id: Arc<ConfigRecordId>,
    ) -> io::Result<Self> {
        let (stop, stopped) = oneshot::channel();
        let (done, finished) = oneshot::channel();
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("anymount-credentials".to_owned())
            .spawn(move || {
                let runtime =
                    match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(runtime) => runtime,
                        Err(error) => {
                            let _ = ready.send(Err(error));
                            return;
                        }
                    };
                let _ = ready.send(Ok(()));
                if let Some(auth) = connector.authorization() {
                    runtime.block_on(watch(
                        auth,
                        repository.as_ref(),
                        &id,
                        stopped,
                    ));
                }
                let _ = done.send(());
            })?;
        let watch = Self {
            stop: Some(stop),
            worker: Some(worker),
            finished,
        };
        started.recv().map_err(io::Error::other)??;
        Ok(watch)
    }

    async fn finish(mut self) {
        self.stop.take();
        let _ = (&mut self.finished).await;
    }
}

impl Drop for CredentialWatch {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            tracing::warn!("credential watcher panicked");
        }
    }
}

async fn watch(
    auth: &dyn Authorization,
    repository: &dyn ConfigRepository,
    id: &ConfigRecordId,
    mut stopped: oneshot::Receiver<()>,
) {
    let mut saved = 0;
    let mut observed = 0;
    loop {
        let stopping = tokio::select! {
            _ = &mut stopped => true,
            generation = auth.wait_for_update(observed) => {
                observed = generation;
                false
            }
        };
        while let Some(update) = auth.credential_snapshot(saved) {
            let generation = update.generation;
            observed = observed.max(generation);
            if repository.save_credentials(id, update).await.is_err() {
                tracing::warn!(
                    generation,
                    stopping,
                    "credential save failed; update retained"
                );
                break;
            }
            saved = generation;
        }
        if stopping {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
