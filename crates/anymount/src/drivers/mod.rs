#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub mod fuse;

pub mod error;
pub mod driver;

pub use error::{Error, Result};
pub use driver::{connect_drivers, connect_drivers_with_telemetry, Driver};
