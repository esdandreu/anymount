//! Probes and control requests for per-provider daemon endpoints.

use crate::daemon::messages::ControlMessage;

#[cfg(unix)]
use crate::daemon::control_unix::UnixControl;

#[cfg(target_os = "windows")]
use crate::daemon::control_windows::WindowsControl;

/// Returns true when the provider process answers `Ping` with `Ready`.
pub fn provider_daemon_ready(provider_name: &str) -> bool {
    matches!(
        send_control_message(provider_name, ControlMessage::Ping),
        Ok(ControlMessage::Ready)
    )
}

/// Send a control message and return the daemon reply.
pub fn send_control_message(
    provider_name: &str,
    message: ControlMessage,
) -> crate::daemon::Result<ControlMessage> {
    send_control_message_impl(provider_name, message)
}

#[cfg(unix)]
fn send_control_message_impl(
    provider_name: &str,
    message: ControlMessage,
) -> crate::daemon::Result<ControlMessage> {
    UnixControl.send(provider_name, message)
}

#[cfg(target_os = "windows")]
fn send_control_message_impl(
    provider_name: &str,
    message: ControlMessage,
) -> crate::daemon::Result<ControlMessage> {
    WindowsControl.send(provider_name, message)
}

#[cfg(not(any(unix, target_os = "windows")))]
fn send_control_message_impl(
    _provider_name: &str,
    _message: ControlMessage,
) -> crate::daemon::Result<ControlMessage> {
    Err(crate::daemon::Error::NotSupported)
}

/// Idempotent shutdown: no error if the daemon is already stopped; returns `Err`
/// only when the daemon answered `Ping` with `Ready` but did not `Ack` shutdown.
pub fn try_disconnect_provider(provider_name: &str) -> std::result::Result<(), String> {
    match send_control_message(provider_name, ControlMessage::Ping) {
        Err(_) => return Ok(()),
        Ok(ControlMessage::Ready) => {}
        Ok(_) => return Ok(()),
    }
    match send_control_message(provider_name, ControlMessage::Shutdown) {
        Ok(ControlMessage::Ack) => Ok(()),
        Ok(other) => Err(format!(
            "shutdown for {provider_name}: expected Ack, got {other:?}"
        )),
        Err(e) => Err(format!("shutdown for {provider_name}: {e}")),
    }
}
