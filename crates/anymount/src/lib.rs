#[cfg(not(target_arch = "wasm32"))]
pub mod config_dir;
pub mod domain;
#[cfg(not(target_arch = "wasm32"))]
pub mod drivers;
#[cfg(not(target_arch = "wasm32"))]
pub mod mount;
#[cfg(not(target_arch = "wasm32"))]
mod read_dir;
#[cfg(not(target_arch = "wasm32"))]
mod read_file_at;
#[cfg(not(target_arch = "wasm32"))]
pub mod storages;
#[cfg(test)]
pub mod test_utils;

#[cfg(not(target_arch = "wasm32"))]
pub use config_dir::ConfigDir;
pub use domain::{Config, ConfigRepository, Storage};
#[cfg(not(target_arch = "wasm32"))]
pub use mount::{MountError, mount};
#[cfg(not(target_arch = "wasm32"))]
pub use read_dir::{ReadDirError, read_dir};
#[cfg(not(target_arch = "wasm32"))]
pub use read_file_at::{ReadFileAtError, read_file_at};
