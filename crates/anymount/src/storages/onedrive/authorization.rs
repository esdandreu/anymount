use oauth2::{
    AuthUrl, ClientId, DeviceAuthorizationUrl, HttpClientError,
    RequestTokenError, Scope, TokenResponse, TokenUrl, basic::BasicClient,
};
use std::thread;
use thiserror::Error;
use url::Url;

use super::{ANYMOUNT_AZURE_APP_CLIENT_ID, OneDriveConfig};
use crate::domain::{AuthStorageError, StartedAuthorization, StorageConfig};

const AUTH_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const TOKEN_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const DEVICE_AUTH_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
const SCOPE_FILES: &str = "Files.ReadWrite";
const SCOPE_OFFLINE: &str = "offline_access";

type HttpError = HttpClientError<ureq::Error>;
type DeviceCodeRequestError =
    RequestTokenError<HttpError, oauth2::basic::BasicErrorResponse>;
type DeviceTokenError =
    RequestTokenError<HttpError, oauth2::DeviceCodeErrorResponse>;
type DeviceCodeOAuthClient = oauth2::Client<
    oauth2::basic::BasicErrorResponse,
    oauth2::basic::BasicTokenResponse,
    oauth2::basic::BasicTokenIntrospectionResponse,
    oauth2::StandardRevocableToken,
    oauth2::basic::BasicRevocationErrorResponse,
    oauth2::EndpointSet,
    oauth2::EndpointSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointSet,
>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid device authorization url: {0}")]
    InvalidDeviceAuthorizationUrl(#[source] url::ParseError),

    #[error("invalid authorization url: {0}")]
    InvalidAuthorizationUrl(#[source] url::ParseError),

    #[error("invalid token url: {0}")]
    InvalidTokenUrl(#[source] url::ParseError),

    #[error("device code request failed")]
    DeviceCodeRequest {
        #[source]
        source: DeviceCodeRequestError,
    },

    #[error("device code expired")]
    DeviceCodeExpired,

    #[error("sign-in was declined")]
    AuthorizationDeclined,

    #[error("token request failed")]
    TokenRequest {
        #[source]
        source: DeviceTokenError,
    },
}

#[derive(Debug)]
struct OneDriveAuthorizer {
    client: DeviceCodeOAuthClient,
    agent: ureq::Agent,
}

impl OneDriveAuthorizer {
    fn new(client_id: Option<String>) -> Result<Self, Error> {
        let client_id = client_id
            .unwrap_or_else(|| ANYMOUNT_AZURE_APP_CLIENT_ID.to_string());
        let device_auth_url =
            DeviceAuthorizationUrl::new(DEVICE_AUTH_URL.to_string())
                .map_err(Error::InvalidDeviceAuthorizationUrl)?;
        let client = BasicClient::new(ClientId::new(client_id))
            .set_auth_uri(
                AuthUrl::new(AUTH_URL.to_string())
                    .map_err(Error::InvalidAuthorizationUrl)?,
            )
            .set_token_uri(
                TokenUrl::new(TOKEN_URL.to_string())
                    .map_err(Error::InvalidTokenUrl)?,
            )
            .set_device_authorization_url(device_auth_url);
        Ok(Self {
            client,
            agent: ureq::Agent::new(),
        })
    }

    fn start_authorization(
        self,
    ) -> Result<ProviderStartedAuthorization, Error> {
        let state = self
            .client
            .exchange_device_code()
            .add_scope(Scope::new(SCOPE_FILES.to_string()))
            .add_scope(Scope::new(SCOPE_OFFLINE.to_string()))
            .request(&self.agent)
            .map_err(|source| Error::DeviceCodeRequest { source })?;
        Ok(ProviderStartedAuthorization {
            authorizer: self,
            state,
        })
    }
}

struct ProviderStartedAuthorization {
    authorizer: OneDriveAuthorizer,
    state: oauth2::StandardDeviceAuthorizationResponse,
}

impl ProviderStartedAuthorization {
    fn wait(&self) -> Result<AuthorizedTokens, Error> {
        let response = self
            .authorizer
            .client
            .exchange_device_access_token(&self.state)
            .request(&self.authorizer.agent, thread::sleep, None)
            .map_err(|source| {
                classify_wait_error(&source.to_string())
                    .unwrap_or(Error::TokenRequest { source })
            })?;
        Ok(AuthorizedTokens {
            access_token: response.access_token().secret().to_string(),
            refresh_token: response
                .refresh_token()
                .map(|token| token.secret().to_string()),
        })
    }

    fn message(&self) -> String {
        format!(
            "To sign in, use a web browser to open {} and enter the code: {}",
            self.verification_uri(),
            self.state.user_code().secret()
        )
    }

    fn verification_uri(&self) -> String {
        if let Some(uri) = self.state.verification_uri_complete() {
            return uri.secret().to_string();
        }
        let base = self.state.verification_uri().to_string();
        let mut url = match Url::parse(&base) {
            Ok(url) => url,
            Err(_) => return base,
        };
        url.query_pairs_mut()
            .append_pair("user_code", self.state.user_code().secret());
        url.to_string()
    }
}

struct AuthorizedTokens {
    access_token: String,
    refresh_token: Option<String>,
}

pub struct OneDriveAuthorization {
    config: OneDriveConfig,
    authorization: ProviderStartedAuthorization,
}

impl OneDriveAuthorization {
    pub fn start(config: OneDriveConfig) -> Result<Self, AuthStorageError> {
        let authorizer = OneDriveAuthorizer::new(config.client_id.clone())
            .map_err(auth_error)?;
        let authorization =
            authorizer.start_authorization().map_err(auth_error)?;
        Ok(Self {
            config,
            authorization,
        })
    }
}

impl StartedAuthorization for OneDriveAuthorization {
    fn message(&self) -> String {
        self.authorization.message()
    }

    fn verification_uri(&self) -> Option<String> {
        Some(self.authorization.verification_uri())
    }

    fn wait(
        self: Box<Self>,
    ) -> Result<Box<dyn StorageConfig>, AuthStorageError> {
        let tokens = self.authorization.wait().map_err(auth_error)?;
        let mut config = self.config;
        config.access_token = Some(tokens.access_token);
        if tokens.refresh_token.is_some() {
            config.refresh_token = tokens.refresh_token;
        }
        Ok(Box::new(config))
    }
}

fn auth_error(error: Error) -> AuthStorageError {
    AuthStorageError::Failed {
        kind: "onedrive",
        message: error.to_string(),
    }
}

fn classify_wait_error(message: &str) -> Option<Error> {
    if message.contains("expired") || message.contains("expired_token") {
        Some(Error::DeviceCodeExpired)
    } else if message.contains("declined")
        || message.contains("authorization_declined")
    {
        Some(Error::AuthorizationDeclined)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_wait_error_is_classified() {
        assert!(matches!(
            classify_wait_error("expired_token"),
            Some(Error::DeviceCodeExpired)
        ));
    }
}
