use crate::daemon::messages::ControlMessage;

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsControl;

impl WindowsControl {
    pub fn bind(&self, _provider_name: &str) -> Result<(), String> {
        Err("windows named-pipe control transport not yet implemented".to_owned())
    }

    pub fn send(
        &self,
        _provider_name: &str,
        _message: ControlMessage,
    ) -> Result<ControlMessage, String> {
        Err("windows named-pipe control transport not yet implemented".to_owned())
    }
}
