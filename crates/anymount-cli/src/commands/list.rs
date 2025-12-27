use clap::Args;
use anyhow::Result;

#[derive(Args, Debug)]
pub struct ListCommand {}

impl ListCommand {
    pub async fn execute(&self) -> Result<()> {
        tracing::info!("Listing active mounts");

        #[cfg(target_os = "macos")]
        {
            use anymount_macos::MacOSMount;

            match MacOSMount::list_all_mounts().await {
                Ok(domains) => {
                    if domains.is_empty() {
                        println!("No active anymount FileProvider domains found.");
                    } else {
                        println!("Active FileProvider domains:");
                        for domain in domains {
                            println!("  • {} ({})", domain.display_name, domain.identifier);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to list domains: {}", e);
                    println!("\nNote: This requires FileProvider extension setup.");
                    println!("See crates/anymount-macos/FILEPROVIDER.md for details.");
                }
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            println!("Mount listing not yet implemented for this platform");
            println!("\nTip: Use platform-specific tools:");
            println!("  Linux: mount | grep anymount");
            println!("  Windows: Check File Explorer");
        }

        Ok(())
    }
}
