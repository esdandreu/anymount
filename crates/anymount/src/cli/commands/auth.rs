use crate::application::auth::{
    Application as AuthApplication, AuthFlow, AuthUseCase, Error as AuthApplicationError,
    Result as AuthApplicationResult, StartedAuthSession,
};
use crate::auth::{self, OneDriveAuthorizer, OneDriveStartedAuthorization, TokenResponse};
use clap::Subcommand;

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
    fn client_id(&self) -> Option<String> {
        match self {
            AuthSubcommand::OneDrive(args) => args.client_id.clone(),
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
        let flow = OneDriveAuthFlow;
        let app = AuthApplication::new(&flow);
        self._execute(&app, &DefaultUrlOpener)
    }

    /// Internal entry point for injection (e.g. tests). Not part of the public
    /// API.
    pub(crate) fn _execute<U, O>(&self, use_case: &U, url_opener: &O) -> crate::cli::Result<()>
    where
        U: AuthUseCase,
        O: UrlOpener,
    {
        let started = use_case
            .start_onedrive_auth(self.subcommand.client_id())
            .map_err(map_auth_error)?;
        print_instructions(&started.message());
        if url_opener.open(&started.verification_uri()).is_err() {
            eprintln!("(Could not open browser; open the URL above manually.)");
        }
        eprintln!();
        eprintln!("Waiting for you to sign in...");
        let tokens = started.finish().map_err(map_auth_error)?;
        print_tokens(&tokens);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct OneDriveAuthFlow;

impl AuthFlow for OneDriveAuthFlow {
    fn start(
        &self,
        client_id: Option<String>,
    ) -> AuthApplicationResult<Box<dyn StartedAuthSession>> {
        let authorizer = OneDriveAuthorizer::new(client_id).map_err(auth::Error::from)?;
        let started =
            OneDriveAuthorizer::start_authorization(authorizer).map_err(auth::Error::from)?;
        Ok(Box::new(started))
    }
}

impl StartedAuthSession for OneDriveStartedAuthorization {
    fn message(&self) -> String {
        OneDriveStartedAuthorization::message(self)
    }

    fn verification_uri(&self) -> String {
        OneDriveStartedAuthorization::verification_uri(self)
    }

    fn finish(self: Box<Self>) -> AuthApplicationResult<TokenResponse> {
        OneDriveStartedAuthorization::wait(self.as_ref())
            .map_err(auth::Error::from)
            .map_err(Into::into)
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

fn map_auth_error(error: AuthApplicationError) -> crate::cli::Error {
    match error {
        AuthApplicationError::Auth(source) => crate::cli::Error::Auth(source),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::auth::{
        AuthUseCase, Error as AuthApplicationError, Result as AuthApplicationResult,
        StartedAuthSession,
    };

    struct MockSession;

    impl StartedAuthSession for MockSession {
        fn message(&self) -> String {
            "Open example.com".to_owned()
        }

        fn verification_uri(&self) -> String {
            "https://example.com/device".to_owned()
        }

        fn finish(self: Box<Self>) -> AuthApplicationResult<TokenResponse> {
            Ok(TokenResponse {
                access_token: "at".into(),
                refresh_token: Some("rt".into()),
                expires_in: 3600,
            })
        }
    }

    struct FailingUseCase;

    impl AuthUseCase for FailingUseCase {
        fn start_onedrive_auth(
            &self,
            _client_id: Option<String>,
        ) -> AuthApplicationResult<Box<dyn StartedAuthSession>> {
            Err(AuthApplicationError::Auth(auth::Error::OneDrive(
                crate::auth::onedrive::Error::DeviceCodeExpired,
            )))
        }
    }

    struct SuccessUseCase;

    impl AuthUseCase for SuccessUseCase {
        fn start_onedrive_auth(
            &self,
            _client_id: Option<String>,
        ) -> AuthApplicationResult<Box<dyn StartedAuthSession>> {
            Ok(Box::new(MockSession))
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
        let err = cmd._execute(&FailingUseCase, &NoOpUrlOpener).unwrap_err();
        assert!(matches!(err, crate::cli::Error::Auth(_)));
    }

    #[test]
    fn execute_succeeds_with_mock_authorizer() {
        let cmd = AuthCommand {
            subcommand: AuthSubcommand::OneDrive(AuthOneDrive {
                client_id: Some("test-client".into()),
            }),
        };
        let result = cmd._execute(&SuccessUseCase, &NoOpUrlOpener);
        assert!(result.is_ok());
    }
}
