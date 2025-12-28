use anymount::Result;
use anymount::windows::{register_sync_root, unregister_sync_root, is_sync_root_registered, RegistrationConfig, HydrationPolicy, CloudFilterCallbacks, populate_root_directory};
use anymount::providers::MockProvider;
use cloud_filter::root::Session;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,anymount_windows=debug".into())
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Anymount Windows Cloud Provider v{}", env!("CARGO_PKG_VERSION"));

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() > 1 && args[1] == "unregister" {
        // Unregister mode
        let sync_root = if args.len() > 2 {
            PathBuf::from(&args[2])
        } else {
            PathBuf::from(r"C:\Users\Public\Anymount")
        };
        
        tracing::info!("Unregistering sync root: {:?}", sync_root);
        
        match unregister_sync_root(&sync_root) {
            Ok(_) => {
                tracing::info!("Sync root unregistered successfully");
            }
            Err(e) => {
                let err_str = format!("{:?}", e);
                if err_str.contains("0x8007017C") {
                    tracing::error!("\nTroubleshooting steps:");
                    tracing::error!("1. Close all File Explorer windows showing the sync root");
                    tracing::error!("2. Run the clean_registry.ps1 script with -dryRun:$false");
                    tracing::error!("   Example: .\\clean_registry.ps1 -dryRun:$false");
                    tracing::error!("3. If the issue persists, restart your computer");
                }
                return Err(e);
            }
        }
        
        return Ok(());
    }
    
    if args.len() > 1 && args[1] == "force-unregister" {
        // Force unregister mode - cleanup registry directly
        let sync_root = if args.len() > 2 {
            PathBuf::from(&args[2])
        } else {
            PathBuf::from(r"C:\Users\Public\Anymount")
        };
        
        tracing::warn!("Force-unregistering sync root: {:?}", sync_root);
        tracing::warn!("This will attempt to clean up the registry directly.");
        tracing::warn!("Please ensure all File Explorer windows are closed.");
        
        // Try normal unregister first
        match unregister_sync_root(&sync_root) {
            Ok(_) => {
                tracing::info!("Sync root unregistered successfully");
                return Ok(());
            }
            Err(_) => {
                tracing::warn!("Normal unregister failed, attempting registry cleanup...");
                tracing::info!("\nPlease run the following PowerShell command as Administrator:");
                tracing::info!("  .\\clean_registry.ps1 -dryRun:$false");
                tracing::info!("\nThis will remove the sync root registration from the Windows registry.");
                return Err(anymount::Error::Platform(
                    "Force unregister requires manual registry cleanup. See instructions above.".to_string()
                ));
            }
        }
    }

    // Default: Register and run mode
    let sync_root = PathBuf::from(r"C:\Users\Public\Anymount");
    
    let config = RegistrationConfig {
        sync_root_path: sync_root.clone(),
        display_name: "Anymount".to_string(),
        provider_id: "com.anymount.windows".to_string(),
        provider_version: env!("CARGO_PKG_VERSION").to_string(),
        icon_resource: None,
        show_overlays: true,
        auto_populate: true,
        hydration_policy: HydrationPolicy::Progressive,
    };

    // Check if already registered
    match is_sync_root_registered(&sync_root) {
        Ok(true) => {
            tracing::info!("Sync root is already registered, skipping registration");
        }
        Ok(false) | Err(_) => {
            tracing::info!("Registering sync root...");
            match register_sync_root(&config) {
                Ok(_) => tracing::info!("✓ Sync root registered successfully"),
                Err(e) => {
                    // Error 0x8007018B means "already registered" or permissions issue
                    let err_str = format!("{:?}", e);
                    if err_str.contains("0x8007018B") || err_str.contains("already") {
                        tracing::info!("Sync root appears to be already registered, continuing...");
                    } else {
                        tracing::error!("Failed to register sync root: {}", e);
                        tracing::error!("Make sure you're running as Administrator");
                        return Err(e);
                    }
                }
            }
        }
    }

    // Initialize mock storage provider
    tracing::info!("Initializing mock storage provider...");
    let mock_provider = MockProvider::new();
    let provider: Arc<dyn anymount::StorageProvider> = Arc::new(mock_provider);

    // Create a Tokio runtime for handling async operations in callbacks
    tracing::info!("Creating async runtime for callbacks...");
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime")
    );

    // Create session and connect with callbacks
    tracing::info!("Connecting to sync root with callbacks...");
    let session = Session::new();
    let callbacks = CloudFilterCallbacks::new(provider.clone(), session, runtime.clone());
    
    let connection = match session.connect(&sync_root, callbacks) {
        Ok(conn) => {
            tracing::info!("✓ Connected to sync root successfully");
            conn
        }
        Err(e) => {
            tracing::error!("Failed to connect to sync root: {:?}", e);
            tracing::error!("Make sure you're running as Administrator");
            let err_str = format!("{:?}", e);
            if err_str.contains("0x8007017A") {
                tracing::error!("\nThe sync root is already connected by another process.");
                tracing::error!("Try running: target\\release\\anymount-win.exe unregister");
                tracing::error!("Or use: target\\release\\anymount-win.exe force-unregister");
            }
            return Err(anymount::Error::Platform(format!(
                "Failed to connect: {:?}",
                e
            )));
        }
    };

    // Populate root directory with placeholder files
    tracing::info!("Creating initial placeholder files...");
    match runtime.block_on(populate_root_directory(&sync_root, provider.clone())) {
        Ok(_) => tracing::info!("✓ Root directory populated"),
        Err(e) => {
            tracing::warn!("Failed to populate root directory: {}", e);
            tracing::warn!("Files may not appear until you access the folder");
        }
    }

    tracing::info!("");
    tracing::info!("Anymount is running. The sync folder is available at:");
    tracing::info!("  {:?}", sync_root);
    tracing::info!("");
    tracing::info!("Open the folder in File Explorer to see the mock files.");
    tracing::info!("");
    tracing::info!("To stop and unregister: {} unregister", args[0]);
    tracing::info!("Press Ctrl+C to stop (sync root will remain registered)");
    tracing::info!("");

    // Keep the application running - use the runtime to wait for Ctrl+C
    runtime.block_on(async {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
    });

    tracing::info!("Shutting down...");
    drop(connection);
    tracing::info!("Note: Sync root remains registered. Run with 'unregister' to remove.");

    Ok(())
}
