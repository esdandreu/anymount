use super::ProviderControl;
use crate::service::control::messages::ControlMessage;
use crate::service::control::unix::UnixControl;

#[derive(Debug, Clone, Copy, Default)]
pub struct ProviderControlUnix;

impl ProviderControl for ProviderControlUnix {
    fn send(
        &self,
        provider_name: &str,
        message: ControlMessage,
    ) -> crate::service::Result<ControlMessage> {
        UnixControl.send(provider_name, message)
    }
}
