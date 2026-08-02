use oauth2::{
    ClientId, HttpClientError, RefreshToken, RequestTokenError, TokenResponse,
    TokenUrl, basic::BasicClient,
};
use parking_lot::RwLock;
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

use super::{ANYMOUNT_AZURE_APP_CLIENT_ID, DEFAULT_TOKEN_EXPIRY_BUFFER_SECS};

const TOKEN_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/token";

type HttpError = HttpClientError<ureq::Error>;
type RefreshTokenRequestError =
    RequestTokenError<HttpError, oauth2::basic::BasicErrorResponse>;
type TokenSourceClient = oauth2::Client<
    oauth2::basic::BasicErrorResponse,
    oauth2::basic::BasicTokenResponse,
    oauth2::basic::BasicTokenIntrospectionResponse,
    oauth2::StandardRevocableToken,
    oauth2::basic::BasicRevocationErrorResponse,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointSet,
>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid token url: {0}")]
    InvalidTokenUrl(#[source] url::ParseError),

    #[error("missing refresh token")]
    MissingRefreshToken,

    #[error("refresh token request failed")]
    RefreshTokenRequest {
        #[source]
        source: RefreshTokenRequestError,
    },
}

#[derive(Debug)]
struct Token {
    refresh_token: Option<String>,
    access_token: Option<String>,
    expires_at: Option<SystemTime>,
}

/// Supplies valid OneDrive access tokens.
#[derive(Debug)]
pub struct OneDriveTokenSource {
    client: TokenSourceClient,
    agent: ureq::Agent,
    token: RwLock<Token>,
    token_expiry_buffer_secs: u64,
}

impl OneDriveTokenSource {
    /// Creates a token source from stored credentials.
    ///
    /// # Errors
    ///
    /// Returns an error when the token endpoint is invalid.
    pub fn new(
        refresh_token: Option<String>,
        access_token: Option<String>,
        client_id: Option<String>,
        token_expiry_buffer_secs: Option<u64>,
    ) -> Result<Self, Error> {
        let client_id = client_id
            .unwrap_or_else(|| ANYMOUNT_AZURE_APP_CLIENT_ID.to_string());
        let client = BasicClient::new(ClientId::new(client_id)).set_token_uri(
            TokenUrl::new(TOKEN_URL.to_string())
                .map_err(Error::InvalidTokenUrl)?,
        );
        let expires_at = access_token.as_deref().and_then(jwt_expires_at);
        let buffer = token_expiry_buffer_secs
            .unwrap_or(DEFAULT_TOKEN_EXPIRY_BUFFER_SECS);
        Ok(Self {
            client,
            agent: ureq::Agent::new(),
            token: RwLock::new(Token {
                refresh_token,
                access_token,
                expires_at,
            }),
            token_expiry_buffer_secs: buffer,
        })
    }

    /// Returns a valid access token, refreshing when needed.
    ///
    /// # Errors
    ///
    /// Returns an error when no refresh token exists or refreshing fails.
    pub fn access_token(&self) -> Result<String, Error> {
        let now = SystemTime::now();
        let buffer = Duration::from_secs(self.token_expiry_buffer_secs);
        {
            let token = self.token.read();
            let valid = token
                .expires_at
                .map(|expiry| expiry > now + buffer)
                .unwrap_or(false);
            if valid && let Some(access_token) = &token.access_token {
                return Ok(access_token.clone());
            }
        }
        self.refresh_access_token()
    }

    fn refresh_access_token(&self) -> Result<String, Error> {
        let mut token = self.token.write();
        let refresh_token = token
            .refresh_token
            .clone()
            .ok_or(Error::MissingRefreshToken)?;
        let response = self
            .client
            .exchange_refresh_token(&RefreshToken::new(refresh_token))
            .request(&self.agent)
            .map_err(|source| Error::RefreshTokenRequest { source })?;
        let expires_in_secs = response
            .expires_in()
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let access_token = response.access_token().secret().to_string();
        token.access_token = Some(access_token.clone());
        token.expires_at =
            Some(SystemTime::now() + Duration::from_secs(expires_in_secs));
        if let Some(refresh_token) = response.refresh_token() {
            token.refresh_token = Some(refresh_token.secret().to_string());
        }
        Ok(access_token)
    }
}

pub fn jwt_expires_at(access_token: &str) -> Option<SystemTime> {
    #[derive(Deserialize)]
    struct ExpClaim {
        exp: Option<u64>,
    }

    let token_data = jsonwebtoken::dangerous::insecure_decode::<ExpClaim>(
        access_token.as_bytes(),
    )
    .ok()?;
    let expiry = token_data.claims.exp?;
    Some(UNIX_EPOCH + Duration::from_secs(expiry))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_source_accepts_refresh_token_only() {
        assert!(
            OneDriveTokenSource::new(Some("rt".into()), None, None, None)
                .is_ok()
        );
    }

    #[test]
    fn access_token_without_refresh_token_returns_error() {
        let source = OneDriveTokenSource::new(None, None, None, None)
            .expect("construct token source");

        assert!(matches!(
            source.access_token(),
            Err(Error::MissingRefreshToken)
        ));
    }

    #[test]
    fn jwt_expiry_is_decoded() -> Result<(), Box<dyn std::error::Error>> {
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJleHAiOjIwMDAwMDAwMDB9.c2ln";
        let expiry = jwt_expires_at(token).ok_or("missing expiry")?;

        assert_eq!(expiry.duration_since(UNIX_EPOCH)?.as_secs(), 2_000_000_000);
        Ok(())
    }
}
