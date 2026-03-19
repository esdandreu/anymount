use super::ProviderControl;
use crate::daemon::control_windows::WindowsControl;
use crate::daemon::messages::ControlMessage;

#[derive(Debug, Clone, Copy, Default)]
pub struct ProviderControlWindows;

impl ProviderControl for ProviderControlWindows {
    fn send(
        &self,
        provider_name: &str,
        message: ControlMessage,
    ) -> crate::daemon::Result<ControlMessage> {
        WindowsControl.send(provider_name, message)
    }
}
