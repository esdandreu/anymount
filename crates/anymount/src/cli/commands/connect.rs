use crate::{ProviderConfiguration, ProvidersConfiguration, get_providers};
use clap::Args;
use std::path::PathBuf;
use std::sync::mpsc;
use tracing::{error, info};

#[derive(Args, Debug, Clone)]
pub struct ConnectCommand {
    /// Path to the mount point
    #[arg(long, default_value = r"C:\Users\Public\Anymount-Test")]
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
    pub async fn execute(&self) -> Result<(), String> {
        let providers = get_providers(self)?;
        for provider in providers {
            provider.connect()?;
            info!(
                "Connected to {} at {}",
                provider.kind(),
                provider.path().display()
            );
        }
        wait_for_ctrlc();
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
