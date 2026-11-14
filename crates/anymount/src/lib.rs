#[cfg(not(target_arch = "wasm32"))]
pub mod config_dir;
pub mod domain;
pub mod drivers;
pub mod mount;
mod read_dir;
mod read_file_at;
pub mod storages;
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[path = "test_utils/http_server.rs"]
pub mod test_http_server;
#[cfg(test)]
pub mod test_utils;

#[cfg(not(target_arch = "wasm32"))]
pub use config_dir::ConfigDir;
pub use domain::{
    Config, ConfigRepository, ConnectPermissionError, DirEntry,
    DisconnectStorageError, ReadDirError, ReadFileAtError, Storage,
    StorageEntry, StoragePath, WriteFileError,
};
pub use mount::{MountError, mount};
pub use read_dir::read_dir;
pub use read_file_at::read_file_at;
