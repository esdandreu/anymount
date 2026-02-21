use crate::auth::token_response::jwt_expires_at;
use crate::auth::refresh_access_token;
use crate::error::Error;
use parking_lot::RwLock;
use serde::Deserialize;
use std::path::{Component, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::storage::{DirEntry, Storage, WriteAt};

/// Default buffer (seconds) before token expiry to trigger refresh when not set in config.
const DEFAULT_TOKEN_EXPIRY_BUFFER_SECS: u64 = 60;

/// OneDrive storage configuration.
///
/// At least one of `access_token` or `refresh_token` must be set. If only
/// `access_token` is set it must not be expired. Optional `client_id` defaults
/// to the built-in Azure app when refreshing. Optional `token_expiry_buffer_secs`
/// defaults to 60.
#[derive(Clone, Debug)]
pub struct OneDriveConfig {
    pub root: PathBuf,
    pub endpoint: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub client_id: Option<String>,
    /// Seconds before expiry to treat token as expired; default to 60 when
    /// not set.
    pub token_expiry_buffer_secs: Option<u64>,
}

impl OneDriveConfig {
    /// Validates the config and creates the storage.
    ///
    /// # Errors
    ///
    /// Returns `InvalidConfig` if neither token is set or access_token is expired
    /// without a refresh_token.
    pub fn connect(self) -> Result<OneDriveStorage, Error> {
        let has_access = self.access_token.is_some();
        let has_refresh = self.refresh_token.is_some();
        if !has_access && !has_refresh {
            return Err(Error::InvalidConfig(
                "OneDrive requires access_token or refresh_token".into(),
            ));
        }
        let buffer_secs = self
            .token_expiry_buffer_secs
            .unwrap_or(DEFAULT_TOKEN_EXPIRY_BUFFER_SECS);
        if has_access && !has_refresh {
            let token = self.access_token.as_deref().unwrap();
            if let Some(exp) = jwt_expires_at(token) {
                let now = SystemTime::now();
                let buffer = Duration::from_secs(buffer_secs);
                if exp <= now + buffer {
                    return Err(Error::InvalidConfig(
                        "access_token is expired and no refresh_token provided".into(),
                    ));
                }
            }
        }
        let endpoint = self.endpoint.trim_end_matches('/').to_string();
        let token_state = RwLock::new(TokenState {
            access_token: self.access_token.clone(),
            expires_at: self
                .access_token
                .as_ref()
                .and_then(|t| jwt_expires_at(t)),
        });
        Ok(OneDriveStorage {
            root: self.root,
            endpoint,
            token_state,
            refresh_token: self.refresh_token,
            client_id: self.client_id,
            token_expiry_buffer_secs: buffer_secs,
        })
    }
}

struct TokenState {
    access_token: Option<String>,
    expires_at: Option<SystemTime>,
}

pub struct OneDriveStorage {
    root: PathBuf,
    endpoint: String,
    token_state: RwLock<TokenState>,
    refresh_token: Option<String>,
    client_id: Option<String>,
    token_expiry_buffer_secs: u64,
}

impl OneDriveStorage {
    /// Ensures we have a valid access token; refreshes if needed.
    fn ensure_token(&self) -> Result<String, String> {
        let mut state = self.token_state.write();
        let now = SystemTime::now();
        let buffer = Duration::from_secs(self.token_expiry_buffer_secs);
        let need_refresh = state.access_token.is_none()
            || state
                .expires_at
                .map(|exp| exp <= now + buffer)
                .unwrap_or(true);
        if need_refresh {
            let refresh_token = match self.refresh_token.as_deref() {
                Some(rt) => rt,
                None => {
                    return Err(
                        "access token expired or missing and no refresh_token available".to_string(),
                    );
                }
            };
            let response = refresh_access_token(self.client_id.as_deref(), refresh_token)
                .map_err(|e| format!("token refresh failed: {}", e))?;
            let access_token = response.access_token.clone();
            state.access_token = Some(access_token.clone());
            state.expires_at = Some(now + Duration::from_secs(response.expires_in));
            return Ok(access_token);
        }
        Ok(state.access_token.as_ref().unwrap().clone())
    }

    fn path_to_graph_segment(path: &PathBuf) -> String {
        if path.as_os_str().is_empty() {
            return "".to_string();
        }
        let has_normal = path.components().any(|c| matches!(c, Component::Normal(_)));
        if !has_normal {
            return "".to_string();
        }
        let s = path.to_string_lossy();
        let s = s.trim_start_matches('/').replace('\\', "/");
        let s = s.trim_end_matches('/');
        if s.is_empty() || s == "." {
            return "".to_string();
        }
        format!("/{}", urlencoding::encode(s).into_owned())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphDriveItem {
    name: String,
    size: Option<u64>,
    #[serde(default)]
    folder: Option<serde_json::Value>,
    last_modified_date_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphChildrenResponse {
    value: Vec<GraphDriveItem>,
}

pub struct OneDriveDirEntry {
    file_name: String,
    is_dir: bool,
    size: u64,
    accessed: SystemTime,
}

impl DirEntry for OneDriveDirEntry {
    fn file_name(&self) -> String {
        self.file_name.clone()
    }
    fn is_dir(&self) -> bool {
        self.is_dir
    }
    fn size(&self) -> u64 {
        self.size
    }
    fn accessed(&self) -> SystemTime {
        self.accessed
    }
}

fn parse_last_modified(s: &str) -> SystemTime {
    // ISO 8601 like "2024-01-15T12:00:00Z"; fallback to UNIX_EPOCH on parse failure.
    let s = s.trim();
    if s.len() >= 19 {
        if let (Some(y), Some(m), Some(d), Some(h), Some(min), Some(sec)) = (
            s.get(0..4).and_then(|x| x.parse::<u64>().ok()),
            s.get(5..7).and_then(|x| x.parse::<u64>().ok()),
            s.get(8..10).and_then(|x| x.parse::<u64>().ok()),
            s.get(11..13).and_then(|x| x.parse::<u64>().ok()),
            s.get(14..16).and_then(|x| x.parse::<u64>().ok()),
            s.get(17..19).and_then(|x| x.parse::<u64>().ok()),
        ) {
            if (1..=12).contains(&m) && (1..=31).contains(&d) && h < 24 && min < 60 && sec < 60 {
                let secs_since_epoch = (y as i64 - 1970) * 365 * 86400
                    + (m as i64 - 1) * 31 * 86400
                    + (d as i64 - 1) * 86400
                    + h as i64 * 3600
                    + min as i64 * 60
                    + sec as i64;
                if secs_since_epoch >= 0 {
                    let d = Duration::from_secs(secs_since_epoch as u64);
                    return UNIX_EPOCH + d;
                }
            }
        }
    }
    UNIX_EPOCH
}

impl Storage for OneDriveStorage {
    type Entry = OneDriveDirEntry;
    type Iter = std::vec::IntoIter<OneDriveDirEntry>;

    fn read_dir(&self, path: PathBuf) -> std::result::Result<Self::Iter, String> {
        let token = self.ensure_token()?;
        let full_path = if path.as_os_str().is_empty() {
            self.root.clone()
        } else {
            self.root.join(path)
        };
        let segment = Self::path_to_graph_segment(&full_path);
        let url = if segment.is_empty() {
            format!("{}/me/drive/root/children", self.endpoint)
        } else {
            format!("{}/me/drive/root:{}:/children", self.endpoint, segment)
        };
        let response = ureq::get(&url)
            .set("Authorization", &format!("Bearer {}", token))
            .call()
            .map_err(|e| format!("OneDrive list failed: {}", e))?;
        if response.status() != 200 {
            let status = response.status();
            let text = response
                .into_string()
                .unwrap_or_else(|_| String::from("(invalid body)"));
            return Err(format!("OneDrive list failed: HTTP {} {}", status, text));
        }
        let parsed: GraphChildrenResponse = serde_json::from_reader(response.into_reader())
            .map_err(|e| format!("OneDrive list response invalid: {}", e))?;
        let entries: Vec<OneDriveDirEntry> = parsed
            .value
            .into_iter()
            .map(|item| {
                let is_dir = item.folder.is_some();
                let size = item.size.unwrap_or(0);
                let accessed = item
                    .last_modified_date_time
                    .as_deref()
                    .map(parse_last_modified)
                    .unwrap_or(UNIX_EPOCH);
                OneDriveDirEntry {
                    file_name: item.name,
                    is_dir,
                    size,
                    accessed,
                }
            })
            .collect();
        Ok(entries.into_iter())
    }

    fn read_file_at(
        &self,
        path: PathBuf,
        writer: &mut impl WriteAt,
        range: std::ops::Range<u64>,
    ) -> std::result::Result<(), String> {
        let token = self.ensure_token()?;
        let full_path = self.root.join(path);
        let segment = Self::path_to_graph_segment(&full_path);
        let url = format!("{}/me/drive/root:{}:/content", self.endpoint, segment);
        let range_header = format!("bytes={}-{}", range.start, range.end.saturating_sub(1));
        let response = ureq::get(&url)
            .set("Authorization", &format!("Bearer {}", token))
            .set("Range", &range_header)
            .call()
            .map_err(|e| format!("OneDrive download failed: {}", e))?;
        let status = response.status();
        if status != 200 && status != 206 {
            let text = response
                .into_string()
                .unwrap_or_else(|_| String::from("(invalid body)"));
            return Err(format!("OneDrive download failed: HTTP {} {}", status, text));
        }
        let mut reader = response.into_reader();
        let mut pos = range.start;
        let mut buf = [0u8; 65536];
        loop {
            let n = std::io::Read::read(&mut reader, &mut buf)
                .map_err(|e| format!("OneDrive read error: {}", e))?;
            if n == 0 {
                break;
            }
            writer
                .write_at(&buf[..n], pos)
                .map_err(|e| format!("write_at failed: {}", e))?;
            pos += n as u64;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_fails_with_no_token() {
        let config = OneDriveConfig {
            root: PathBuf::from("/"),
            endpoint: "https://graph.microsoft.com/v1.0".into(),
            access_token: None,
            refresh_token: None,
            client_id: None,
            token_expiry_buffer_secs: None,
        };
        let r = config.connect();
        let Err(e) = r else { panic!("expected config to fail") };
        assert!(e.to_string().contains("access_token or refresh_token"));
    }

    #[test]
    fn config_ok_with_refresh_token_without_client_id() {
        let config = OneDriveConfig {
            root: PathBuf::from("/"),
            endpoint: "https://graph.microsoft.com/v1.0".into(),
            access_token: None,
            refresh_token: Some("rt".into()),
            client_id: None,
            token_expiry_buffer_secs: None,
        };
        let r = config.connect();
        assert!(r.is_ok());
    }

    #[test]
    fn config_ok_with_refresh_token_and_client_id() {
        let config = OneDriveConfig {
            root: PathBuf::from("/"),
            endpoint: "https://graph.microsoft.com/v1.0".into(),
            access_token: None,
            refresh_token: Some("rt".into()),
            client_id: Some("cid".into()),
            token_expiry_buffer_secs: None,
        };
        let r = config.connect();
        assert!(r.is_ok());
    }

    #[test]
    fn config_ok_with_valid_access_token() {
        let payload_b64 = "eyJleHAiOjI1MDAwMDAwMDB9"; // exp far in future
        let token = format!("h.{}.sig", payload_b64);
        let config = OneDriveConfig {
            root: PathBuf::from("/"),
            endpoint: "https://graph.microsoft.com/v1.0".into(),
            access_token: Some(token),
            refresh_token: None,
            client_id: None,
            token_expiry_buffer_secs: None,
        };
        let r = config.connect();
        assert!(r.is_ok());
    }

    #[test]
    fn config_fails_expired_access_token_without_refresh() {
        // Valid JWT with exp in the past so jwt_expires_at returns Some(past time).
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJleHAiOjEwMDAwMDAwMDB9.c2ln";
        let config = OneDriveConfig {
            root: PathBuf::from("/"),
            endpoint: "https://graph.microsoft.com/v1.0".into(),
            access_token: Some(token.to_string()),
            refresh_token: None,
            client_id: None,
            token_expiry_buffer_secs: None,
        };
        let r = config.connect();
        let Err(e) = r else { panic!("expected config to fail") };
        assert!(e.to_string().contains("expired"));
    }

    #[test]
    fn path_to_graph_segment() {
        assert_eq!(
            OneDriveStorage::path_to_graph_segment(&PathBuf::from("")),
            ""
        );
        assert_eq!(
            OneDriveStorage::path_to_graph_segment(&PathBuf::from(".")),
            ""
        );
        assert_eq!(
            OneDriveStorage::path_to_graph_segment(&PathBuf::from("/.")),
            ""
        );
        assert_eq!(
            OneDriveStorage::path_to_graph_segment(&PathBuf::from("Docs")),
            "/Docs"
        );
        assert!(OneDriveStorage::path_to_graph_segment(&PathBuf::from("a/b")).contains("a"));
    }
}
