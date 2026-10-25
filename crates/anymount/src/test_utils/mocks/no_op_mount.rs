use crate::domain::{
    ConnectMountError, DisconnectMountError, Mount, UnregisterMountError,
};
use async_trait::async_trait;
use std::path::Path;

pub struct NoOpMount;

#[async_trait]
impl Mount for NoOpMount {
    fn name(&self) -> &str {
        "no-op"
    }

    fn root(&self) -> &Path {
        Path::new("/tmp/no-op")
    }

    fn is_connected(&self) -> bool {
        true
    }

    async fn connect(&self) -> Result<(), ConnectMountError> {
        Ok(())
    }

    async fn disconnect(&self) -> Result<(), DisconnectMountError> {
        Ok(())
    }

    async fn unregister(&self) -> Result<(), UnregisterMountError> {
        Ok(())
    }
}
