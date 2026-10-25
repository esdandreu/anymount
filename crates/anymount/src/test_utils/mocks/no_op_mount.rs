use crate::domain::{
    ConnectMountError, DisconnectMountError, Mount, UnregisterMountError,
};
use std::path::Path;

pub struct NoOpMount;

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

    fn connect(&self) -> Result<(), ConnectMountError> {
        Ok(())
    }

    fn disconnect(&self) -> Result<(), DisconnectMountError> {
        Ok(())
    }

    fn unregister(&self) -> Result<(), UnregisterMountError> {
        Ok(())
    }
}
