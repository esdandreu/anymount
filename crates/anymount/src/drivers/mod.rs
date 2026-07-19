mod init_driver_or_default;

pub use init_driver_or_default::{
    NoDefaultDriver, get_default_driver, init_driver_or_default,
};

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(feature = "fuse")]
pub mod fuse;
