#[cfg(test)]
mod create_sparse_file;
mod get_default_driver_config;
mod init_driver;

#[cfg(test)]
pub use create_sparse_file::create_sparse_file;
pub use get_default_driver_config::{
    NoDefaultDriver, get_default_driver_config,
};
pub use init_driver::init_driver;

#[cfg(target_arch = "wasm32")]
pub mod browser;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(feature = "fuse")]
pub mod fuse;
