use clap::Parser;
use std::process::{ExitCode, Termination};

#[repr(u8)]
pub enum AnymountResult {
    Ok = 0,
    Err = 1,
}

impl Termination for AnymountResult {
    fn report(self) -> ExitCode {
        // Maybe print a message here
        ExitCode::from(self as u8)
    }
}

fn main() -> AnymountResult {
    match anymount::Cli::parse().run() {
        Ok(()) => AnymountResult::Ok,
        Err(_) => AnymountResult::Err,
    }
}
