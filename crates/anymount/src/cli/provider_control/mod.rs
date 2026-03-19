//! Probes and control requests for per-provider service endpoints.

use crate::service::control::messages::ControlMessage;

#[cfg(unix)]
mod provider_control_unix;
#[cfg(target_os = "windows")]
mod provider_control_windows;

#[cfg(unix)]
pub use provider_control_unix::ProviderControlUnix;
#[cfg(target_os = "windows")]
pub use provider_control_windows::ProviderControlWindows;

/// Sends control messages to a named provider service (Unix socket or Windows named pipe).
pub trait ProviderControl {
    fn send(
        &self,
        provider_name: &str,
        message: ControlMessage,
    ) -> crate::service::Result<ControlMessage>;

    fn provider_daemon_ready(&self, provider_name: &str) -> bool {
        matches!(
            self.send(provider_name, ControlMessage::Ping),
            Ok(ControlMessage::Ready)
        )
    }

    /// Idempotent shutdown: no error if the service is already stopped; returns `Err`
    /// only when the service answered `Ping` with `Ready` but did not `Ack` shutdown.
    fn try_disconnect_provider(&self, provider_name: &str) -> std::result::Result<(), String> {
        match self.send(provider_name, ControlMessage::Ping) {
            Err(_) => return Ok(()),
            Ok(ControlMessage::Ready) => {}
            Ok(_) => return Ok(()),
        }
        match self.send(provider_name, ControlMessage::Shutdown) {
            Ok(ControlMessage::Ack) => Ok(()),
            Ok(other) => Err(format!(
                "shutdown for {provider_name}: expected Ack, got {other:?}"
            )),
            Err(e) => Err(format!("shutdown for {provider_name}: {e}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProviderControlUnsupported;

impl ProviderControl for ProviderControlUnsupported {
    fn send(
        &self,
        _provider_name: &str,
        _message: ControlMessage,
    ) -> crate::service::Result<ControlMessage> {
        Err(crate::service::Error::NotSupported)
    }
}

#[cfg(unix)]
type Platform = ProviderControlUnix;
#[cfg(target_os = "windows")]
type Platform = ProviderControlWindows;
#[cfg(not(any(unix, target_os = "windows")))]
type Platform = ProviderControlUnsupported;

fn platform() -> Platform {
    Platform::default()
}

/// Returns true when the provider service answers `Ping` with `Ready`.
pub fn provider_daemon_ready(provider_name: &str) -> bool {
    platform().provider_daemon_ready(provider_name)
}

/// Send a control message and return the service reply.
pub fn send_control_message(
    provider_name: &str,
    message: ControlMessage,
) -> crate::service::Result<ControlMessage> {
    platform().send(provider_name, message)
}

pub fn try_disconnect_provider(provider_name: &str) -> std::result::Result<(), String> {
    platform().try_disconnect_provider(provider_name)
}
