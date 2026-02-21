use jsonwebtoken::dangerous::insecure_decode;
use oauth2::{
    basic::{BasicClient, BasicTokenResponse}, AuthUrl, ClientId, DeviceAuthorizationUrl,
    RefreshToken, Scope, StandardDeviceAuthorizationResponse, TokenResponse as OAuth2TokenResponse,
    TokenUrl,
};
use serde::Deserialize;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const DEVICE_AUTH_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
const SCOPE_FILES: &str = "Files.ReadWrite";
const SCOPE_OFFLINE: &str = "offline_access";


/// Response from the device-code endpoint.
///
/// Show `message` (or `verification_uri` and `user_code`) to the user; use
/// `device_code` and `interval` when calling `poll_for_tokens`.
#[derive(Debug, Clone)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub message: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Token endpoint response (success or error).
///
/// On success, `access_token` and `refresh_token` are `Some`. On pending or
/// error, `error` and `error_description` may be set.
#[derive(Debug, Clone)]
pub struct TokenResponse {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// Opaque state from `request_device_code`; pass to `poll_for_tokens`.
pub struct DeviceCodeState(StandardDeviceAuthorizationResponse);

fn http_client() -> ureq::Agent {
    ureq::Agent::new()
}

/// Requests a device code from Microsoft.
///
/// Returns user-facing `DeviceCodeResponse` and a `DeviceCodeState` to pass to
/// `poll_for_tokens`. Show `message` (or `verification_uri` and `user_code`) to
/// the user, then call `poll_for_tokens` with the state.
///
/// # Errors
///
/// Returns an error if the HTTP request fails, the response is not 200, or
/// the response body is not valid.
pub fn request_device_code(
    client_id: &str,
) -> Result<(DeviceCodeResponse, DeviceCodeState), String> {
    let device_auth_url = DeviceAuthorizationUrl::new(DEVICE_AUTH_URL.to_string())
        .map_err(|e| format!("Invalid device authorization URL: {}", e))?;
    let client = BasicClient::new(ClientId::new(client_id.to_string()))
        .set_auth_uri(AuthUrl::new(AUTH_URL.to_string()).map_err(|e| format!("Invalid auth URL: {}", e))?)
        .set_token_uri(TokenUrl::new(TOKEN_URL.to_string()).map_err(|e| format!("Invalid token URL: {}", e))?)
        .set_device_authorization_url(device_auth_url);
    let agent = http_client();
    let details: StandardDeviceAuthorizationResponse = client
        .exchange_device_code()
        .add_scope(Scope::new(SCOPE_FILES.to_string()))
        .add_scope(Scope::new(SCOPE_OFFLINE.to_string()))
        .request(&agent)
        .map_err(|e| format!("Device code request failed: {}", e))?;

    let verification_uri = details.verification_uri().to_string();
    let user_code = details.user_code().secret().to_string();
    let message = format!(
        "To sign in, use a web browser to open {} and enter the code: {}",
        verification_uri, user_code
    );
    let response = DeviceCodeResponse {
        device_code: details.device_code().secret().to_string(),
        user_code: user_code.clone(),
        verification_uri: verification_uri.clone(),
        message,
        expires_in: details.expires_in().as_secs(),
        interval: details.interval().as_secs().max(1),
    };
    Ok((response, DeviceCodeState(details)))
}

/// Polls the token endpoint until the user completes sign-in or the code expires.
///
/// Pass the `DeviceCodeState` returned from `request_device_code`. The oauth2
/// crate uses the interval from the device response. Returns a `TokenResponse`
/// with tokens on success.
///
/// # Errors
///
/// Returns an error if the device code expires, the user declines, the
/// response is invalid, or the token response lacks access or refresh token.
pub fn poll_for_tokens(
    client_id: &str,
    state: DeviceCodeState,
) -> Result<TokenResponse, String> {
    let device_auth_url = DeviceAuthorizationUrl::new(DEVICE_AUTH_URL.to_string())
        .map_err(|e| format!("Invalid device authorization URL: {}", e))?;
    let client = BasicClient::new(ClientId::new(client_id.to_string()))
        .set_auth_uri(AuthUrl::new(AUTH_URL.to_string()).map_err(|e| format!("Invalid auth URL: {}", e))?)
        .set_token_uri(TokenUrl::new(TOKEN_URL.to_string()).map_err(|e| format!("Invalid token URL: {}", e))?)
        .set_device_authorization_url(device_auth_url);
    let agent = http_client();
    let token_result = client
        .exchange_device_access_token(&state.0)
        .request(&agent, thread::sleep, None)
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

    Ok(token_response_from_standard(&token_result))
}

fn token_response_from_standard(r: &BasicTokenResponse) -> TokenResponse {
    TokenResponse {
        access_token: Some(r.access_token().secret().to_string()),
        refresh_token: r
            .refresh_token()
            .map(|t| t.secret().to_string())
            .into(),
        expires_in: r.expires_in().map(|d| d.as_secs()).into(),
        error: None,
        error_description: None,
    }
}

/// Exchanges a refresh token for a new access token.
///
/// Uses the Microsoft Entra token endpoint with `grant_type=refresh_token`.
/// On success returns a `TokenResponse` with at least `access_token`; Microsoft
/// may also return a new `refresh_token` and `expires_in`.
///
/// # Errors
///
/// Returns an error if the HTTP request fails, the response is not 200, the
/// body is not valid, or the response contains an OAuth error or lacks
/// an access_token.
pub fn refresh_access_token(
    client_id: &str,
    refresh_token: &str,
) -> Result<TokenResponse, String> {
    let client = BasicClient::new(ClientId::new(client_id.to_string()))
        .set_auth_uri(AuthUrl::new(AUTH_URL.to_string()).map_err(|e| format!("Invalid auth URL: {}", e))?)
        .set_token_uri(TokenUrl::new(TOKEN_URL.to_string()).map_err(|e| format!("Invalid token URL: {}", e))?);
    let agent = http_client();
    let token_result = client
        .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
        .request(&agent)
        .map_err(|e| format!("Refresh token request failed: {}", e))?;

    Ok(token_response_from_standard(&token_result))
}

/// Reads the `exp` claim from a JWT access token payload.
///
/// Decodes without signature verification. Use only to check expiry for
/// validation; do not use for security decisions that rely on token integrity.
///
/// Returns `None` if the token is invalid, or the `exp` claim is missing or
/// invalid.
pub fn jwt_expires_at(access_token: &str) -> Option<SystemTime> {
    #[derive(Deserialize)]
    struct ExpClaim {
        exp: Option<u64>,
    }
    let token_data = insecure_decode::<ExpClaim>(access_token.as_bytes()).ok()?;
    let exp = token_data.claims.exp?;
    Some(UNIX_EPOCH + Duration::from_secs(exp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_code_response_shape() {
        let r = DeviceCodeResponse {
            device_code: "dc".into(),
            user_code: "uc".into(),
            verification_uri: "https://microsoft.com/devicelogin".into(),
            message: "open https://microsoft.com/devicelogin".into(),
            expires_in: 900,
            interval: 5,
        };
        assert_eq!(r.interval, 5);
        assert!(r.message.contains("devicelogin"));
    }

    #[test]
    fn token_response_success_shape() {
        let r = TokenResponse {
            access_token: Some("at".into()),
            refresh_token: Some("rt".into()),
            expires_in: Some(3600),
            error: None,
            error_description: None,
        };
        assert!(r.access_token.is_some());
        assert!(r.refresh_token.is_some());
    }

    #[test]
    fn token_response_error_shape() {
        let r = TokenResponse {
            access_token: None,
            refresh_token: None,
            expires_in: None,
            error: Some("authorization_pending".into()),
            error_description: Some("pending".into()),
        };
        assert_eq!(r.error.as_deref(), Some("authorization_pending"));
    }

    #[test]
    fn jwt_expires_at_valid() {
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJleHAiOjIwMDAwMDAwMDB9.c2ln";
        let t = jwt_expires_at(token).unwrap();
        assert_eq!(t.duration_since(UNIX_EPOCH).unwrap().as_secs(), 2000000000);
    }

    #[test]
    fn jwt_expires_at_invalid_shape() {
        assert!(jwt_expires_at("not-three-parts").is_none());
        assert!(jwt_expires_at("a.b").is_none());
    }

    #[test]
    fn jwt_expires_at_missing_exp() {
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.e30.c2ln";
        assert!(jwt_expires_at(token).is_none());
    }
}
