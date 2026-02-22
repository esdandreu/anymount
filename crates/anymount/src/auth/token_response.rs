use oauth2::basic::BasicTokenResponse;
use oauth2::TokenResponse as OAuth2TokenResponse;
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Token endpoint response (success).
#[derive(Debug, Clone)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
}

impl From<BasicTokenResponse> for TokenResponse {
    fn from(r: BasicTokenResponse) -> Self {
        let access_token = r.access_token().secret().to_string();
        let expires_in = r
            .expires_in()
            .map(|d| d.as_secs())
            .or_else(|| {
                jwt_expires_at(&access_token).and_then(|exp_at| {
                    exp_at
                        .duration_since(SystemTime::now())
                        .ok()
                        .map(|d| d.as_secs())
                })
            })
            .unwrap_or(0);
        TokenResponse {
            access_token,
            refresh_token: r.refresh_token().map(|t| t.secret().to_string()),
            expires_in,
        }
    }
}

/// Reads the `exp` claim from a JWT access token payload.
///
/// Decodes without signature verification. Used to populate `expires_in` when
/// the token endpoint omits it and for expiry checks in storage.
pub(crate) fn jwt_expires_at(access_token: &str) -> Option<SystemTime> {
    #[derive(Deserialize)]
    struct ExpClaim {
        exp: Option<u64>,
    }
    let token_data = jsonwebtoken::dangerous::insecure_decode::<ExpClaim>(access_token.as_bytes()).ok()?;
    let exp = token_data.claims.exp?;
    Some(UNIX_EPOCH + Duration::from_secs(exp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_expires_at_valid() -> Result<(), Box<dyn std::error::Error>> {
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJleHAiOjIwMDAwMDAwMDB9.c2ln";
        let t = jwt_expires_at(token).ok_or("missing exp")?;
        assert_eq!(t.duration_since(UNIX_EPOCH)?.as_secs(), 2000000000);
        Ok(())
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
