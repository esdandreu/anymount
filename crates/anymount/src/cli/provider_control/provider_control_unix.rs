use super::ProviderControl;
use crate::daemon::control_unix::UnixControl;
use crate::daemon::messages::ControlMessage;

#[derive(Debug, Clone, Copy, Default)]
pub struct ProviderControlUnix;

impl ProviderControl for ProviderControlUnix {
    fn send(
        &self,
        provider_name: &str,
        message: ControlMessage,
    ) -> crate::daemon::Result<ControlMessage> {
        UnixControl.send(provider_name, message)
    }
}
