#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod fuse;

#[cfg(target_os = "macos")]
pub use driver::MacosDriver;

pub mod error;
pub mod driver;

pub use error::{Error, Result};
pub use driver::{connect_drivers, connect_drivers_with_telemetry, Driver};
