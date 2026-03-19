use crate::auth::TokenResponse;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Auth(#[from] crate::auth::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait StartedAuthSession {
    fn message(&self) -> String;
    fn verification_uri(&self) -> String;
    fn finish(self: Box<Self>) -> Result<TokenResponse>;
}

pub trait AuthFlow {
    fn start(&self, client_id: Option<String>) -> Result<Box<dyn StartedAuthSession>>;
}

pub trait AuthUseCase {
    fn start_onedrive_auth(&self, client_id: Option<String>)
    -> Result<Box<dyn StartedAuthSession>>;
}

pub struct Application<'a, F> {
    flow: &'a F,
}

impl<'a, F> Application<'a, F> {
    pub fn new(flow: &'a F) -> Self {
        Self { flow }
    }
}

impl<F> AuthUseCase for Application<'_, F>
where
    F: AuthFlow,
{
    fn start_onedrive_auth(
        &self,
        client_id: Option<String>,
    ) -> Result<Box<dyn StartedAuthSession>> {
        self.flow.start(client_id)
    }
}

#[cfg(test)]
mod tests {
    use super::{Application, AuthFlow, AuthUseCase, Result, StartedAuthSession};
    use crate::auth::TokenResponse;

    struct TestSession {
        message: String,
        verification_uri: String,
        tokens: TokenResponse,
    }

    impl StartedAuthSession for TestSession {
        fn message(&self) -> String {
            self.message.clone()
        }

        fn verification_uri(&self) -> String {
            self.verification_uri.clone()
        }

        fn finish(self: Box<Self>) -> Result<TokenResponse> {
            Ok(self.tokens)
        }
    }

    #[derive(Default)]
    struct TestAuthFlow {
        refresh_token: Option<String>,
        access_token: Option<String>,
    }

    impl AuthFlow for TestAuthFlow {
        fn start(&self, _client_id: Option<String>) -> Result<Box<dyn StartedAuthSession>> {
            Ok(Box::new(TestSession {
                message: "open https://example.test/device".to_owned(),
                verification_uri: "https://example.test/device".to_owned(),
                tokens: TokenResponse {
                    access_token: self.access_token.clone().unwrap_or_default(),
                    refresh_token: self.refresh_token.clone(),
                    expires_in: 3600,
                },
            }))
        }
    }

    struct TestAuthApp {
        flow: TestAuthFlow,
    }

    impl TestAuthApp {
        fn with_tokens(mut self, refresh_token: &str, access_token: &str) -> Self {
            self.flow.refresh_token = Some(refresh_token.to_owned());
            self.flow.access_token = Some(access_token.to_owned());
            self
        }

        fn start_onedrive_auth(
            &self,
            client_id: Option<String>,
        ) -> Result<Box<dyn StartedAuthSession>> {
            self.application().start_onedrive_auth(client_id)
        }

        fn application(&self) -> Application<'_, TestAuthFlow> {
            Application::new(&self.flow)
        }
    }

    fn test_auth_app() -> TestAuthApp {
        TestAuthApp {
            flow: TestAuthFlow::default(),
        }
    }

    #[test]
    fn auth_returns_instructions_and_tokens() {
        let app = test_auth_app().with_tokens("refresh", "access");
        let started = app.start_onedrive_auth(None).expect("auth should start");

        assert!(started.message().contains("open"));
        let tokens = started.finish().expect("auth should finish");
        assert_eq!(tokens.refresh_token.as_deref(), Some("refresh"));
    }
}
