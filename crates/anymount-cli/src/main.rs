use clap::Parser;
use std::process::{ExitCode, Termination};

mod cli;
mod commands;

use cli::Cli;

#[repr(u8)]
pub enum AnymountResult {
    Ok = 0,
    Err = 1,
}

impl Termination for AnymountResult {
    fn report(self) -> ExitCode {
        ExitCode::from(self as u8)
    }
}

#[tokio::main]
async fn main() -> AnymountResult {
    match Cli::parse().run().await {
        Ok(()) => AnymountResult::Ok,
        Err(_) => AnymountResult::Err,
    }
}
