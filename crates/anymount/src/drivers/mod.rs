#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(any(target_os = "linux", feature = "macos"))]
pub mod fuse;

#[cfg(feature = "macos")]
pub use driver::MacosDriver;

pub mod error;
pub mod driver;

pub use error::{Error, Result};
pub use driver::{connect_drivers, connect_drivers_with_telemetry, Driver};
