pub mod callbacks;
pub mod cleanup_registry;
pub mod error;
pub mod placeholders;
pub mod register;
pub mod windows_driver;

pub use crate::drivers::Driver;
pub use crate::storages::Storage;
pub use callbacks::Callbacks;
pub use cleanup_registry::cleanup_registry;
pub use error::{Error, Result};
pub use register::{HydrationPolicy, RegistrationConfig};
pub use windows_driver::WindowsDriver;
