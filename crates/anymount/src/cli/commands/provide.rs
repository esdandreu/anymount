use clap::Args;
use std::path::PathBuf;

#[derive(Args, Debug, Clone)]
pub struct ProvideCommand {
    #[arg(long)]
    pub name: String,

    #[arg(long)]
    pub config_dir: Option<PathBuf>,
}

impl ProvideCommand {
    pub fn execute(&self) -> Result<(), String> {
        Err("provide command not yet implemented".to_owned())
    }
}
