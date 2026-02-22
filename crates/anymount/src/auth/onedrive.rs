use super::token_response::{TokenResponse, jwt_expires_at};
pub use oauth2::StandardDeviceAuthorizationResponse;
use oauth2::{
    AuthUrl, ClientId, DeviceAuthorizationUrl, RefreshToken, Scope,
    TokenResponse as OAuth2TokenResponse, TokenUrl, basic::BasicClient,
};
use parking_lot::RwLock;
use std::thread;
use std::time::{Duration, SystemTime};
use url::Url;

const AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const DEVICE_AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
const SCOPE_FILES: &str = "Files.ReadWrite";
const SCOPE_OFFLINE: &str = "offline_access";

const ANYMOUNT_AZURE_APP_CLIENT_ID: &str = "5970173e-1b75-4317-987d-6849236cc3df";
const DEFAULT_TOKEN_EXPIRY_BUFFER_SECS: u64 = 60;

/// OAuth client configured with auth, token, and device-authorization URLs.
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

/// Authorizer that uses the Microsoft device-code flow. Client ID is baked in.
#[derive(Debug)]
pub struct OneDriveAuthorizer {
    client: DeviceCodeOAuthClient,
    agent: ureq::Agent,
}

impl OneDriveAuthorizer {
    pub fn new(client_id: Option<String>) -> Result<Self, String> {
        let client_id = client_id.unwrap_or_else(|| ANYMOUNT_AZURE_APP_CLIENT_ID.to_string());
        let device_auth_url = DeviceAuthorizationUrl::new(DEVICE_AUTH_URL.to_string())
            .map_err(|e| format!("Invalid device authorization URL: {}", e))?;
        let client = BasicClient::new(ClientId::new(client_id))
            .set_auth_uri(
                AuthUrl::new(AUTH_URL.to_string())
                    .map_err(|e| format!("Invalid auth URL: {}", e))?,
            )
            .set_token_uri(
                TokenUrl::new(TOKEN_URL.to_string())
                    .map_err(|e| format!("Invalid token URL: {}", e))?,
            )
            .set_device_authorization_url(device_auth_url);
        Ok(Self {
            client,
            agent: ureq::Agent::new(),
        })
    }

    /// Starts the device-code flow; returns a value to show the user and wait for tokens.
    pub fn start_authorization(self) -> Result<OneDriveStartedAuthorization, String> {
        let state = self
            .client
            .exchange_device_code()
            .add_scope(Scope::new(SCOPE_FILES.to_string()))
            .add_scope(Scope::new(SCOPE_OFFLINE.to_string()))
            .request(&self.agent)
            .map_err(|e| format!("Device code request failed: {}", e))?;
        Ok(OneDriveStartedAuthorization {
            authorizer: self,
            state,
        })
    }
}

/// Started OneDrive device-code flow. Composes an [`OneDriveAuthorizer`] with device state and display strings.
pub struct OneDriveStartedAuthorization {
    authorizer: OneDriveAuthorizer,
    state: StandardDeviceAuthorizationResponse,
}

impl OneDriveStartedAuthorization {
    /// Blocks until the user completes sign-in and returns the token response.
    pub fn wait(&self) -> Result<TokenResponse, String> {
        let token_result = self
            .authorizer
            .client
            .exchange_device_access_token(&self.state)
            .request(&self.authorizer.agent, thread::sleep, None)
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("expired") || msg.contains("expired_token") {
                    "Device code expired. Please run the command again.".to_string()
                } else if msg.contains("declined") || msg.contains("authorization_declined") {
                    "Sign-in was declined.".to_string()
                } else {
                    format!("Token request failed: {}", e)
                }
            })?;
        Ok(TokenResponse::from(token_result))
    }

    /// User-facing message (e.g. "To sign in, open ... and enter the code: ...").
    pub fn message(&self) -> String {
        format!(
            "To sign in, use a web browser to open {} and enter the code: {}",
            self.verification_uri(),
            self.state.user_code().secret()
        )
    }

    /// URI for the user to open in a browser, with user_code as a query parameter.
    pub fn verification_uri(&self) -> String {
        if let Some(uri) = self.state.verification_uri_complete() {
            return uri.secret().to_string();
        }
        let base = self.state.verification_uri().to_string();
        let mut url = match Url::parse(&base) {
            Ok(u) => u,
            Err(_) => return base,
        };
        url.query_pairs_mut()
            .append_pair("user_code", self.state.user_code().secret());
        url.to_string()
    }
}

#[derive(Debug)]
struct Token {
    refresh_token: Option<String>,
    access_token: Option<String>,
    expires_at: Option<SystemTime>,
}

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

/// Holds OAuth client, agent, and token state; returns a valid access token,
/// refreshing when needed.
#[derive(Debug)]
pub struct OneDriveTokenSource {
    client: TokenSourceClient,
    agent: ureq::Agent,
    token: RwLock<Token>,
    token_expiry_buffer_secs: u64,
}

impl OneDriveTokenSource {
    pub fn new(
        refresh_token: Option<String>,
        access_token: Option<String>,
        client_id: Option<String>,
        token_expiry_buffer_secs: Option<u64>,
    ) -> Result<Self, String> {
        let client_id = client_id.unwrap_or_else(|| ANYMOUNT_AZURE_APP_CLIENT_ID.to_string());
        let client = BasicClient::new(ClientId::new(client_id)).set_token_uri(
            TokenUrl::new(TOKEN_URL.to_string())
                .map_err(|e| format!("Invalid token URL: {}", e))?,
        );
        let expires_at = access_token.as_deref().and_then(jwt_expires_at);
        let buffer = token_expiry_buffer_secs.unwrap_or(DEFAULT_TOKEN_EXPIRY_BUFFER_SECS);
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

    /// Returns a valid access token, refreshing with the refresh token if
    /// needed.
    pub fn access_token(&self) -> Result<String, String> {
        let now = SystemTime::now();
        let buffer = Duration::from_secs(self.token_expiry_buffer_secs);
        {
            let token = self.token.read();
            let valid = token
                .expires_at
                .map(|exp| exp > now + buffer)
                .unwrap_or(false);
            if valid {
                if let Some(ref at) = token.access_token {
                    return Ok(at.clone());
                }
            }
        }
        self.refresh_access_token()
    }

    fn refresh_access_token(&self) -> Result<String, String> {
        let mut token = self.token.write();
        let refresh_token = token.refresh_token.clone().ok_or_else(|| {
            "access token expired or missing and no refresh_token available".to_string()
        })?;
        let response = self
            .client
            .exchange_refresh_token(&RefreshToken::new(refresh_token))
            .request(&self.agent)
            .map_err(|e| format!("Refresh token request failed: {}", e))?;
        let expires_in_secs = response.expires_in().map(|d| d.as_secs()).unwrap_or(0);
        let access_token = response.access_token().secret().to_string();
        token.access_token = Some(access_token.clone());
        token.expires_at = Some(SystemTime::now() + Duration::from_secs(expires_in_secs));
        if let Some(rt) = response.refresh_token() {
            token.refresh_token = Some(rt.secret().to_string());
        }
        Ok(access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_response_success_shape() {
        let r = TokenResponse {
            access_token: "at".into(),
            refresh_token: Some("rt".into()),
            expires_in: 3600,
        };
        assert_eq!(r.access_token, "at");
        assert!(r.refresh_token.is_some());
    }

    #[test]
    fn token_source_new_with_refresh_token_only() {
        let source = OneDriveTokenSource::new(Some("rt".into()), None, None, None);
        assert!(source.is_ok());
    }

    #[test]
    fn token_source_new_with_access_token_only() {
        let payload_b64 = "eyJleHAiOjI1MDAwMDAwMDB9";
        let token = format!("h.{}.sig", payload_b64);
        let source = OneDriveTokenSource::new(None, Some(token), None, None);
        assert!(source.is_ok());
    }

    #[test]
    fn token_source_new_with_client_id_default() {
        let source = OneDriveTokenSource::new(Some("rt".into()), None, None, None);
        assert!(source.is_ok());
    }

    #[test]
    fn token_source_new_with_explicit_client_id() {
        let source = OneDriveTokenSource::new(
            Some("rt".into()),
            None,
            Some("custom-client-id".into()),
            None,
        );
        assert!(source.is_ok());
    }
}
