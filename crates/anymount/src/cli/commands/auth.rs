use crate::auth::{DeviceCodeResponse, DeviceCodeState, OneDriveAuthorizer, TokenResponse};
use clap::Subcommand;
use std::result::Result;

/// Auth subcommand: which provider to obtain a token for.
#[derive(Subcommand, Debug, Clone)]
pub enum AuthSubcommand {
    /// Obtain a refresh token for OneDrive (device code flow).
    #[command(name = "onedrive")]
    OneDrive(AuthOneDrive),
}

/// Arguments for `auth onedrive`.
#[derive(clap::Args, Debug, Clone)]
pub struct AuthOneDrive {
    /// Override the default Azure app client ID.
    #[arg(long)]
    pub client_id: Option<String>,
}

impl AuthSubcommand {
    fn authorizer(&self) -> Result<impl DeviceCodeAuthorizer, String> {
        match self {
            AuthSubcommand::OneDrive(args) => OneDriveAuthorizer::new(args.client_id.clone()),
        }
    }
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
        let authorizer = self.subcommand.authorizer()?;
        self._execute(&authorizer, &DefaultUrlOpener)
    }

    /// Internal entry point for injection (e.g. tests). Not part of the public
    /// API.
    pub(crate) fn _execute<A, U>(&self, authorizer: &A, url_opener: &U) -> Result<(), String>
    where
        A: DeviceCodeAuthorizer,
        U: UrlOpener,
    {
        let (device, state) = authorizer.request_device_code()?;
        print_instructions(&device);
        if url_opener.open(&device.verification_uri).is_err() {
            eprintln!("(Could not open browser; open the URL above manually.)");
        }
        eprintln!();
        eprintln!("Waiting for you to sign in...");
        let tokens = authorizer.poll_for_tokens(state)?;
        print_tokens(&tokens);
        Ok(())
    }
}

/// Port for opening a URL (e.g. in the default browser). Inject a no-op in tests.
pub trait UrlOpener {
    fn open(&self, url: &str) -> Result<(), ()>;
}

/// Default opener that uses the system handler (e.g. opens the default
/// browser).
pub struct DefaultUrlOpener;

impl UrlOpener for DefaultUrlOpener {
    fn open(&self, url: &str) -> Result<(), ()> {
        open::that(url).map_err(|_| ())
    }
}

pub trait DeviceCodeAuthorizer {
    /// Opaque state returned from
    /// [`request_device_code`](DeviceCodeAuthorizer::request_device_code) and
    /// passed to [`poll_for_tokens`](DeviceCodeAuthorizer::poll_for_tokens).
    type State;

    fn request_device_code(&self) -> Result<(DeviceCodeResponse, Self::State), String>;

    fn poll_for_tokens(&self, state: Self::State) -> Result<TokenResponse, String>;
}

impl DeviceCodeAuthorizer for OneDriveAuthorizer {
    type State = DeviceCodeState;

    fn request_device_code(&self) -> Result<(DeviceCodeResponse, DeviceCodeState), String> {
        self.request_device_code()
    }

    fn poll_for_tokens(&self, state: DeviceCodeState) -> Result<TokenResponse, String> {
        self.poll_for_tokens(state)
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::onedrive::{DeviceCodeResponse, TokenResponse};

    struct FailingAuthorizer;

    impl DeviceCodeAuthorizer for FailingAuthorizer {
        type State = ();

        fn request_device_code(&self) -> Result<(DeviceCodeResponse, ()), String> {
            Err("mock device code error".into())
        }

        fn poll_for_tokens(&self, _state: ()) -> Result<TokenResponse, String> {
            Err("mock poll error".into())
        }
    }

    struct NoOpUrlOpener;

    impl UrlOpener for NoOpUrlOpener {
        fn open(&self, _url: &str) -> Result<(), ()> {
            Ok(())
        }
    }

    #[test]
    fn execute_returns_authorizer_error_without_real_auth() {
        let cmd = AuthCommand {
            subcommand: AuthSubcommand::OneDrive(AuthOneDrive {
                client_id: Some("test-client".into()),
            }),
        };
        let err = cmd
            ._execute(&FailingAuthorizer, &NoOpUrlOpener)
            .unwrap_err();
        assert_eq!(err, "mock device code error");
    }

    struct SuccessAuthorizer;

    impl DeviceCodeAuthorizer for SuccessAuthorizer {
        type State = ();

        fn request_device_code(&self) -> Result<(DeviceCodeResponse, ()), String> {
            Ok((
                DeviceCodeResponse {
                    device_code: "dc".into(),
                    user_code: "uc".into(),
                    verification_uri: "https://example.com/device".into(),
                    message: "Open example.com".into(),
                    expires_in: 900,
                    interval: 5,
                },
                (),
            ))
        }

        fn poll_for_tokens(&self, _state: ()) -> Result<TokenResponse, String> {
            Ok(TokenResponse {
                access_token: Some("at".into()),
                refresh_token: Some("rt".into()),
                expires_in: Some(3600),
                error: None,
                error_description: None,
            })
        }
    }

    #[test]
    fn execute_succeeds_with_mock_authorizer() {
        let cmd = AuthCommand {
            subcommand: AuthSubcommand::OneDrive(AuthOneDrive {
                client_id: Some("test-client".into()),
            }),
        };
        let result = cmd._execute(&SuccessAuthorizer, &NoOpUrlOpener);
        assert!(result.is_ok());
    }
}
