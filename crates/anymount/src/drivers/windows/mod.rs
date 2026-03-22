pub mod callbacks;
pub mod cleanup_registry;
pub mod error;
pub mod placeholders;
pub mod register;
pub mod windows_driver;

pub(crate) use callbacks::Callbacks;
pub(crate) use cleanup_registry::cleanup_registry;
pub use error::{Error, Result};
pub use register::{HydrationPolicy, RegistrationConfig};
pub use windows_driver::WindowsSession;
