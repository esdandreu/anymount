use super::token_response::TokenResponse;
pub use oauth2::StandardDeviceAuthorizationResponse;
use oauth2::{
    AuthUrl, ClientId, DeviceAuthorizationUrl, RefreshToken, Scope, TokenUrl, basic::BasicClient,
};
use std::thread;

const AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const DEVICE_AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
const SCOPE_FILES: &str = "Files.ReadWrite";
const SCOPE_OFFLINE: &str = "offline_access";

const ANYMOUNT_AZURE_APP_CLIENT_ID: &str = "5970173e-1b75-4317-987d-6849236cc3df";

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
        let uri = state.verification_uri().to_string();
        let message = format!(
            "To sign in, use a web browser to open {} and enter the code: {}",
            uri,
            state.user_code().secret()
        );
        Ok(OneDriveStartedAuthorization {
            authorizer: self,
            state,
            message,
            verification_uri: uri,
        })
    }

    /// Exchanges a refresh token for a new access token.
    ///
    /// Uses the Microsoft Entra token endpoint with `grant_type=refresh_token`.
    /// On success returns a `TokenResponse` with `access_token` and `expires_in`;
    /// Microsoft may also return a new `refresh_token`.
    pub fn refresh_access_token(&self, refresh_token: &str) -> Result<TokenResponse, String> {
        let token_result = self
            .client
            .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
            .request(&self.agent)
            .map_err(|e| format!("Refresh token request failed: {}", e))?;
        Ok(TokenResponse::from(token_result))
    }
}

/// Started OneDrive device-code flow. Composes an [`OneDriveAuthorizer`] with device state and display strings.
pub struct OneDriveStartedAuthorization {
    authorizer: OneDriveAuthorizer,
    state: StandardDeviceAuthorizationResponse,
    message: String,
    verification_uri: String,
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
    pub fn display_message(&self) -> String {
        self.message.clone()
    }

    /// URI for the user to open in a browser.
    pub fn display_verification_uri(&self) -> String {
        self.verification_uri.clone()
    }
}

/// Exchanges a refresh token for a new access token.
///
/// When `client_id` is `None`, uses the default Azure app client ID.
/// Prefer using [`OneDriveAuthorizer::refresh_access_token`] when you have an authorizer.
pub fn refresh_access_token(
    client_id: Option<&str>,
    refresh_token: &str,
) -> Result<TokenResponse, String> {
    let authorizer = OneDriveAuthorizer::new(client_id.map(String::from))?;
    authorizer.refresh_access_token(refresh_token)
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
}
