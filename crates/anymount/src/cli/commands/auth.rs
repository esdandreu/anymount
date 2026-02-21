use crate::auth::onedrive::{self, DeviceCodeResponse, TokenResponse};
use clap::Subcommand;
use std::result::Result;

/// Default Azure app client ID for OneDrive. Overridable via `--client-id`.
const ANYMOUNT_AZURE_APP_CLIENT_ID: &str = "5970173e-1b75-4317-987d-6849236cc3df";

/// Auth subcommand: which provider to obtain a token for.
#[derive(Subcommand, Debug, Clone)]
pub enum AuthSubcommand {
    /// Obtain a refresh token for OneDrive (device code flow).
    Onedrive(OnedriveAuthArgs),
}

/// Arguments for `auth onedrive`.
#[derive(clap::Args, Debug, Clone)]
pub struct OnedriveAuthArgs {
    /// Override the default Azure app client ID.
    #[arg(long, default_value = ANYMOUNT_AZURE_APP_CLIENT_ID)]
    pub client_id: String,
}

/// Top-level auth command; holds the provider subcommand.
#[derive(clap::Args, Debug, Clone)]
pub struct AuthCommand {
    #[command(subcommand)]
    pub subcommand: AuthSubcommand,
}

impl AuthCommand {
    /// Runs the chosen auth flow and prints the refresh token on stdout.
    ///
    /// # Errors
    ///
    /// Returns an error if the device-code request fails, the user does not
    /// complete sign-in, or the token response is invalid.
    pub fn execute(&self) -> Result<(), String> {
        match &self.subcommand {
            AuthSubcommand::Onedrive(args) => run_onedrive_auth(args),
        }
    }
}

fn run_onedrive_auth(args: &OnedriveAuthArgs) -> Result<(), String> {
    let (device, state) = onedrive::request_device_code(&args.client_id)?;
    print_instructions(&device);
    if open::that(&device.verification_uri).is_err() {
        eprintln!("(Could not open browser; open the URL above manually.)");
    }
    eprintln!();
    eprintln!("Waiting for you to sign in...");
    let tokens = onedrive::poll_for_tokens(&args.client_id, state)?;
    print_tokens(&tokens);
    Ok(())
}

fn print_instructions(device: &DeviceCodeResponse) {
    eprintln!("{}", device.message);
}

fn print_tokens(tokens: &TokenResponse) {
    if let Some(ref r) = tokens.refresh_token {
        println!("refresh_token: {}", r);
    }
    if tokens.access_token.is_some() {
        eprintln!("access_token is short-lived; use refresh_token for storage config.");
    }
}
