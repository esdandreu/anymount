use crate::domain::{
    ConnectMountError, DisconnectMountError, Mount, UnregisterMountError,
};
use async_trait::async_trait;
use std::path::Path;
use std::rc::Rc;

#[derive(Default)]
pub struct NoOpMount(Rc<()>);

#[async_trait(?Send)]
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
        tokio::task::yield_now().await;
        let _ = &self.0;
        Ok(())
    }

    async fn disconnect(&self) -> Result<(), DisconnectMountError> {
        Ok(())
    }

    async fn unregister(&self) -> Result<(), UnregisterMountError> {
        Ok(())
    }
}
