use crate::{ProviderConfiguration, ProvidersConfiguration, StorageConfig, connect_providers};
use clap::{Args, Subcommand};
use std::path::PathBuf;
use std::sync::mpsc;
use tracing::{error, info};

#[derive(Args, Debug, Clone)]
pub struct ConnectCommand {
    /// Path to the mount point
    #[arg(long)]
    pub path: PathBuf,
    #[command(subcommand)]
    pub storage: ConnectStorageSubcommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConnectStorageSubcommand {
    /// Local directory as storage backend
    Local(LocalStorageArgs),
}

#[derive(Args, Debug, Clone)]
pub struct LocalStorageArgs {
    /// Root directory to expose
    #[arg(value_name = "ROOT")]
    pub root: PathBuf,
}

impl ConnectStorageSubcommand {
    fn to_storage_config(&self) -> StorageConfig {
        match self {
            Self::Local(args) => StorageConfig::Local {
                root: args.root.clone(),
            },
        }
    }
}

impl ProviderConfiguration for ConnectCommand {
    fn path(&self) -> PathBuf {
        self.path.clone()
    }

    fn storage_config(&self) -> StorageConfig {
        self.storage.to_storage_config()
    }
}

impl ProvidersConfiguration for ConnectCommand {
    fn providers(&self) -> Vec<&impl ProviderConfiguration> {
        vec![self]
    }
}

impl ConnectCommand {
    pub fn execute(&self) -> Result<(), String> {
        let providers = connect_providers(self)?;
        for provider in &providers {
            info!(
                "Connected to {} at {}",
                provider.kind(),
                provider.path().display()
            );
        }
        info!("All providers connected. Press Ctrl+C to disconnect.");
        wait_for_ctrlc();
        // Keep providers alive until here
        drop(providers);
        Ok(())
    }
}

fn wait_for_ctrlc() {
    let (tx, rx) = mpsc::channel();

    if let Err(e) = ctrlc::set_handler(move || {
        let _ = tx.send(());
    }) {
        error!("Error setting Ctrl-C handler: {}", e);
        return;
    }

    let _ = rx.recv();
}
