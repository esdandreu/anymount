pub mod callbacks;
pub mod populate;
pub mod provider;
pub mod registration;
pub mod sync;

pub use callbacks::CloudFilterCallbacks;
pub use populate::populate_root_directory;
pub use provider::WindowsCloudProvider;
pub use registration::{
    register_sync_root, unregister_sync_root, is_sync_root_registered,
    RegistrationConfig, HydrationPolicy
};
pub use sync::SyncEngine;

use crate::Result;

/// Initialize the Windows Cloud Filter integration
pub fn init() -> Result<()> {
    tracing::info!("Initializing Windows Cloud Filter API support");
    Ok(())
}

