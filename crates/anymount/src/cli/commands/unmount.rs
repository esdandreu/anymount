use clap::Args;
use anymount::{Error, Result};

#[derive(Args, Debug)]
pub struct UnmountCommand {
    /// Domain identifier to unmount (e.g., com.anymount.mock)
    pub domain_id: String,
}

impl UnmountCommand {
    pub async fn execute(&self) -> Result<()> {
        tracing::info!("Unmounting domain: {}", self.domain_id);
        
        Err(Error::NotSupported(
            "Unmount not yet implemented. \
             This requires FileProvider extension integration. \
             See crates/anymount-macos/FILEPROVIDER.md".to_string()
        ))
    }
}
