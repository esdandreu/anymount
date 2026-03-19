use super::ProviderControl;
use crate::service::control::messages::ControlMessage;
use crate::service::control::windows::WindowsControl;

#[derive(Debug, Clone, Copy, Default)]
pub struct ProviderControlWindows;

impl ProviderControl for ProviderControlWindows {
    fn send(
        &self,
        provider_name: &str,
        message: ControlMessage,
    ) -> crate::service::Result<ControlMessage> {
        WindowsControl.send(provider_name, message)
    }
}
