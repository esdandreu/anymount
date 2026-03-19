//! Probes per-provider daemon control endpoints for CLI commands.

use crate::daemon::messages::ControlMessage;

#[cfg(unix)]
use crate::daemon::control_unix::UnixControl;

#[cfg(target_os = "windows")]
use crate::daemon::control_windows::WindowsControl;

/// Returns true when the provider process answers `Ping` with `Ready`.
pub fn provider_daemon_ready(provider_name: &str) -> bool {
    provider_daemon_ready_impl(provider_name)
}

#[cfg(unix)]
fn provider_daemon_ready_impl(provider_name: &str) -> bool {
    matches!(
        UnixControl.send(provider_name, ControlMessage::Ping),
        Ok(ControlMessage::Ready)
    )
}

#[cfg(target_os = "windows")]
fn provider_daemon_ready_impl(provider_name: &str) -> bool {
    matches!(
        WindowsControl.send(provider_name, ControlMessage::Ping),
        Ok(ControlMessage::Ready)
    )
}

#[cfg(not(any(unix, target_os = "windows")))]
fn provider_daemon_ready_impl(_provider_name: &str) -> bool {
    false
}
