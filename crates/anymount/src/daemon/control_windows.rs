use crate::daemon::messages::ControlMessage;
use crate::daemon::{Error, Result};

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsControl;

impl WindowsControl {
    pub fn bind(&self, _provider_name: &str) -> Result<()> {
        Err(Error::NotSupported)
    }

    pub fn send(&self, _provider_name: &str, _message: ControlMessage) -> Result<ControlMessage> {
        Err(Error::NotSupported)
    }
}
