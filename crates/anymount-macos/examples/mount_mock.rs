//! Example: Mount a mock storage provider on macOS using FileProvider
//!
//! This example demonstrates how to use anymount-macos to mount a storage
//! provider as a native macOS volume using the FileProvider framework.
//!
//! **Note**: This requires a FileProvider extension to be installed.
//! See FILEPROVIDER.md for details on creating the extension.

use anymount_macos::MacOSMount;
use anymount_providers::MockProvider;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    tracing::info!("Starting anymount macOS FileProvider example");

    // Create a mock storage provider
    // In production, this would be S3Provider, AzureProvider, etc.
    let provider = Arc::new(MockProvider::new());

    // Create a mount with a unique domain ID and display name
    let domain_id = "com.anymount.example.mock";
    let display_name = "Example Mock Storage";

    tracing::info!(
        "Creating mount: {} ({})",
        display_name,
        domain_id
    );

    let mount = MacOSMount::new(provider, domain_id, display_name)?;

    // Mount the storage - this registers it with the FileProvider system
    tracing::info!("Mounting storage...");
    match mount.mount().await {
        Ok(_) => {
            tracing::info!("✅ Storage mounted successfully!");
            tracing::info!("Check Finder sidebar for '{}'", display_name);
            tracing::info!("");
            tracing::info!("Press Ctrl+C to unmount and exit");
        }
        Err(e) => {
            tracing::error!("❌ Failed to mount: {}", e);
            tracing::error!("");
            tracing::error!("This likely means:");
            tracing::error!("  1. FileProvider extension is not installed");
            tracing::error!("  2. Extension is not properly configured");
            tracing::error!("  3. System extension approval is needed");
            tracing::error!("");
            tracing::error!("See FILEPROVIDER.md for setup instructions");
            return Err(e.into());
        }
    }

    // List all currently registered domains
    tracing::info!("");
    tracing::info!("Currently registered FileProvider domains:");
    match MacOSMount::list_all_mounts().await {
        Ok(domains) => {
            for domain in domains {
                tracing::info!(
                    "  - {} ({})",
                    domain.display_name,
                    domain.identifier
                );
            }
        }
        Err(e) => {
            tracing::warn!("Failed to list domains: {}", e);
        }
    }

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;
    tracing::info!("");
    tracing::info!("Shutting down...");

    // Unmount the storage
    mount.unmount().await?;
    tracing::info!("✅ Storage unmounted successfully");

    Ok(())
}

