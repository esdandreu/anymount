pub mod error;
pub mod onedrive;
pub mod token_response;

use crate::application::auth::{
    AuthFlow as ApplicationAuthFlow, Result as ApplicationAuthResult, StartedAuthSession,
};

pub use error::{Error, Result};
pub use onedrive::{
    OneDriveAuthorizer, OneDriveStartedAuthorization, OneDriveTokenSource,
    StandardDeviceAuthorizationResponse,
};
pub use token_response::TokenResponse;

#[derive(Debug, Clone, Copy, Default)]
pub struct OneDriveAuthFlow;

impl ApplicationAuthFlow for OneDriveAuthFlow {
    fn start(
        &self,
        client_id: Option<String>,
    ) -> ApplicationAuthResult<Box<dyn StartedAuthSession>> {
        let authorizer = OneDriveAuthorizer::new(client_id).map_err(Error::from)?;
        let started = authorizer.start_authorization().map_err(Error::from)?;
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

    fn finish(self: Box<Self>) -> ApplicationAuthResult<TokenResponse> {
        OneDriveStartedAuthorization::wait(self.as_ref())
            .map_err(Error::from)
            .map_err(Into::into)
    }
}
