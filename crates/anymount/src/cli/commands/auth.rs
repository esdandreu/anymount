use crate::auth::{self, OneDriveAuthorizer, OneDriveStartedAuthorization, TokenResponse};
use clap::Subcommand;

/// Value returned from [`Authorizer::start_authorization`]; use [`message`](StartedAuthorization::message) and [`verification_uri`](StartedAuthorization::verification_uri) for the user, then [`wait`](StartedAuthorization::wait) to obtain tokens.
pub trait StartedAuthorization {
    fn wait(&self) -> auth::Result<TokenResponse>;
    fn message(&self) -> String;
    fn verification_uri(&self) -> String;
}

/// Starts an authorization flow; returns a [`StartedAuthorization`] to display
/// instructions and wait for completion.
pub trait Authorizer {
    fn start_authorization(self) -> auth::Result<impl StartedAuthorization>;
}

impl StartedAuthorization for OneDriveStartedAuthorization {
    fn wait(&self) -> auth::Result<TokenResponse> {
        self.wait().map_err(auth::Error::from)
    }

    fn message(&self) -> String {
        self.message()
    }

    fn verification_uri(&self) -> String {
        self.verification_uri()
    }
}

impl Authorizer for OneDriveAuthorizer {
    fn start_authorization(self) -> auth::Result<impl StartedAuthorization> {
        OneDriveAuthorizer::start_authorization(self).map_err(auth::Error::from)
    }
}

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
    fn authorizer(&self) -> auth::Result<impl Authorizer> {
        match self {
            AuthSubcommand::OneDrive(args) => {
                OneDriveAuthorizer::new(args.client_id.clone()).map_err(auth::Error::from)
            }
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
    pub fn execute(&self) -> crate::cli::Result<()> {
        let authorizer = self.subcommand.authorizer()?;
        self._execute(authorizer, &DefaultUrlOpener)
    }

    /// Internal entry point for injection (e.g. tests). Not part of the public
    /// API.
    pub(crate) fn _execute<A, U>(&self, authorizer: A, url_opener: &U) -> crate::cli::Result<()>
    where
        A: Authorizer,
        U: UrlOpener,
    {
        let started = authorizer.start_authorization()?;
        print_instructions(&started.message());
        if url_opener.open(&started.verification_uri()).is_err() {
            eprintln!("(Could not open browser; open the URL above manually.)");
        }
        eprintln!();
        eprintln!("Waiting for you to sign in...");
        let tokens = started.wait()?;
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

fn print_instructions(message: &str) {
    eprintln!("{}", message);
}

fn print_tokens(tokens: &TokenResponse) {
    if let Some(ref r) = tokens.refresh_token {
        println!("refresh_token: {}", r);
    }
    eprintln!("access_token is short-lived; use refresh_token for storage config.");
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockStarted;

    impl StartedAuthorization for MockStarted {
        fn wait(&self) -> auth::Result<TokenResponse> {
            Ok(TokenResponse {
                access_token: "at".into(),
                refresh_token: Some("rt".into()),
                expires_in: 3600,
            })
        }

        fn message(&self) -> String {
            "Open example.com".into()
        }

        fn verification_uri(&self) -> String {
            "https://example.com/device".into()
        }
    }

    struct FailingAuthorizer;

    impl Authorizer for FailingAuthorizer {
        fn start_authorization(self) -> auth::Result<impl StartedAuthorization> {
            Err::<MockStarted, _>(auth::Error::OneDrive(
                crate::auth::onedrive::Error::DeviceCodeExpired,
            ))
        }
    }

    struct SuccessAuthorizer;

    impl Authorizer for SuccessAuthorizer {
        fn start_authorization(self) -> auth::Result<impl StartedAuthorization> {
            Ok(MockStarted)
        }
    }

    struct NoOpUrlOpener;

    impl UrlOpener for NoOpUrlOpener {
        fn open(&self, _url: &str) -> Result<(), ()> {
            Ok(())
        }
    }

    #[test]
    fn auth_execute_wraps_auth_error() {
        let cmd = AuthCommand {
            subcommand: AuthSubcommand::OneDrive(AuthOneDrive {
                client_id: Some("test-client".into()),
            }),
        };
        let err = cmd._execute(FailingAuthorizer, &NoOpUrlOpener).unwrap_err();
        assert!(matches!(err, crate::cli::Error::Auth(_)));
    }

    #[test]
    fn execute_succeeds_with_mock_authorizer() {
        let cmd = AuthCommand {
            subcommand: AuthSubcommand::OneDrive(AuthOneDrive {
                client_id: Some("test-client".into()),
            }),
        };
        let result = cmd._execute(SuccessAuthorizer, &NoOpUrlOpener);
        assert!(result.is_ok());
    }
}
