use crate::{ProviderConfiguration, ProvidersConfiguration, connect_providers};
use clap::Args;
use std::path::PathBuf;
use std::sync::mpsc;
use tracing::{error, info};

#[derive(Args, Debug, Clone)]
pub struct ConnectCommand {
    /// Path to the mount point
    #[arg(long)]
    pub path: PathBuf,
}

impl ProviderConfiguration for ConnectCommand {
    fn path(&self) -> PathBuf {
        self.path.clone()
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
