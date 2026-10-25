use crate::{app::MountConfig, mount_service::MountService};
use async_trait::async_trait;

pub struct MockMountService {
    pub mounts: Vec<MountConfig>,
    pub connected: Vec<String>,
    pub disconnected: Vec<String>,
}

impl MockMountService {
    pub fn new(mounts: impl IntoIterator<Item = MountConfig>) -> Self {
        Self {
            mounts: mounts.into_iter().collect(),
            connected: Vec::new(),
            disconnected: Vec::new(),
        }
    }
}

#[async_trait(?Send)]
impl MountService for MockMountService {
    async fn list(&self) -> color_eyre::Result<Vec<MountConfig>> {
        Ok(self.mounts.clone())
    }

    async fn connect(&mut self, name: &str) -> color_eyre::Result<()> {
        self.connected.push(name.to_owned());
        if let Some(mount) =
            self.mounts.iter_mut().find(|mount| mount.name == name)
        {
            mount.is_connected = true;
        }
        Ok(())
    }

    async fn disconnect(&mut self, name: &str) -> color_eyre::Result<()> {
        self.disconnected.push(name.to_owned());
        if let Some(mount) =
            self.mounts.iter_mut().find(|mount| mount.name == name)
        {
            mount.is_connected = false;
        }
        Ok(())
    }
}
