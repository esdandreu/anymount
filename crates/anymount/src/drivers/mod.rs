#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(any(target_os = "linux", feature = "fuse"))]
pub mod fuse;

#[cfg(feature = "fuse")]
pub use fuse::FuseDriver;

pub mod driver;
pub mod error;

pub use driver::{Session, connect_drivers};
pub use error::{Error, Result};
