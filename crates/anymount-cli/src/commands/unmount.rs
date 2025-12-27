use clap::Args;
use anyhow::Result;

#[derive(Args, Debug)]
pub struct UnmountCommand {
    /// Domain identifier to unmount (e.g., com.anymount.mock)
    pub domain_id: String,
}

impl UnmountCommand {
    pub async fn execute(&self) -> Result<()> {
        tracing::info!("Unmounting domain: {}", self.domain_id);
        
        anyhow::bail!(
            "Unmount not yet implemented. \
             This requires FileProvider extension integration. \
             See crates/anymount-macos/FILEPROVIDER.md"
        );
    }
}
