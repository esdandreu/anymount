use crate::auth::onedrive::OneDriveTokenSource;
use crate::auth::token_response::jwt_expires_at;
use crate::error::Error;
use serde::Deserialize;
use std::path::{Component, PathBuf};
use chrono::{DateTime, Utc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::storage::{DirEntry, Storage, WriteAt};

/// Default buffer (seconds) before token expiry to trigger refresh when not set in config.
const DEFAULT_TOKEN_EXPIRY_BUFFER_SECS: u64 = 60;

/// OneDrive storage configuration.
///
/// At least one of `access_token` or `refresh_token` must be set. If only
/// `access_token` is set it must not be expired. Optional `client_id` defaults
/// to the built-in Azure app when refreshing. Optional
/// `token_expiry_buffer_secs` defaults to 60.
#[derive(Clone, Debug)]
pub struct OneDriveConfig {
    pub root: PathBuf,
    pub endpoint: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub client_id: Option<String>,
    pub token_expiry_buffer_secs: Option<u64>,
}

impl OneDriveConfig {
    /// Validates the config and creates the storage.
    ///
    /// # Errors
    ///
    /// Returns `InvalidConfig` if neither token is set or access_token is
    /// expired without a refresh_token.
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
        let token_source = OneDriveTokenSource::new(
            self.refresh_token.clone(),
            self.access_token.clone(),
            self.client_id.clone(),
            self.token_expiry_buffer_secs,
        )
        .map_err(Error::InvalidConfig)?;
        Ok(OneDriveStorage {
            root: self.root,
            endpoint,
            token_source,
        })
    }
}

pub struct OneDriveStorage {
    root: PathBuf,
    endpoint: String,
    token_source: OneDriveTokenSource,
}

impl OneDriveStorage {
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

impl Storage for OneDriveStorage {
    type Entry = OneDriveDirEntry;
    type Iter = std::vec::IntoIter<OneDriveDirEntry>;

    fn read_dir(&self, path: PathBuf) -> std::result::Result<Self::Iter, String> {
        let token = self
            .token_source
            .access_token()
            .map_err(|e| format!("token: {}", e))?;
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
        let token = self
            .token_source
            .access_token()
            .map_err(|e| format!("token: {}", e))?;
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
            return Err(format!(
                "OneDrive download failed: HTTP {} {}",
                status, text
            ));
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

fn parse_last_modified(s: &str) -> SystemTime {
    DateTime::parse_from_rfc3339(s.trim())
        .map(|dt| dt.with_timezone(&Utc).into())
        .unwrap_or(UNIX_EPOCH)
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
        let Err(e) = r else {
            panic!("expected config to fail")
        };
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
        let Err(e) = r else {
            panic!("expected config to fail")
        };
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
