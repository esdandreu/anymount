use crate::auth::onedrive::OneDriveTokenSource;
use crate::auth::token_response::jwt_expires_at;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::path::{Component, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

use super::storage::{DirEntry, Storage, WriteAt};
use super::{Error as StorageError, Result as StorageResult};

/// Default buffer (seconds) before token expiry to trigger refresh when not set in config.
const DEFAULT_TOKEN_EXPIRY_BUFFER_SECS: u64 = 60;
type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid config: {message}")]
    InvalidConfig { message: String },

    #[error("token access failed")]
    Token {
        #[source]
        source: crate::auth::onedrive::Error,
    },

    #[error("request failed for {url}")]
    Request {
        url: String,
        #[source]
        source: BoxError,
    },

    #[error("response body read failed for {url}: {source}")]
    ReadBody {
        url: String,
        #[source]
        source: std::io::Error,
    },

    #[error("list failed for {url} with status {status}: {body}")]
    ListFailed {
        url: String,
        status: u16,
        body: String,
    },

    #[error("list response invalid for {url}: {source}")]
    ListResponse {
        url: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("download failed for {url} with status {status}: {body}")]
    DownloadFailed {
        url: String,
        status: u16,
        body: String,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Provides a bearer token for HTTP requests.
///
/// Implementations may refresh tokens; callers use the returned string
/// in an `Authorization: Bearer <token>` header.
///
/// # Errors
///
/// Returns an error string when the token cannot be obtained or refreshed.
pub trait BearerToken: Send + Sync + 'static {
    fn access_token(&self) -> Result<String>;
}

/// Fetches a URL with headers and returns status and full body.
///
/// Used by OneDrive storage to list and download; mockable for tests.
///
/// # Errors
///
/// Returns an error string on network failure or when the response cannot be read.
pub trait HttpGet: Send + Sync + 'static {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, Vec<u8>)>;
}

/// Production HTTP GET implementation using ureq.
pub struct UreqHttpGet;

impl UreqHttpGet {
    pub fn new() -> Self {
        Self
    }
}

impl Default for UreqHttpGet {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpGet for UreqHttpGet {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, Vec<u8>)> {
        let mut request = ureq::get(url);
        for (k, v) in headers {
            request = request.set(*k, *v);
        }
        let response = request.call().map_err(|source| Error::Request {
            url: url.to_owned(),
            source: Box::new(source),
        })?;
        let status = response.status();
        let mut reader = response.into_reader();
        let mut body = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut body).map_err(|source| Error::ReadBody {
            url: url.to_owned(),
            source,
        })?;
        Ok((status, body))
    }
}

impl BearerToken for OneDriveTokenSource {
    fn access_token(&self) -> Result<String> {
        OneDriveTokenSource::access_token(self).map_err(|source| Error::Token { source })
    }
}

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
    pub fn connect(self) -> StorageResult<OneDriveStorage> {
        let has_access = self.access_token.is_some();
        let has_refresh = self.refresh_token.is_some();
        if !has_access && !has_refresh {
            return Err(StorageError::OneDrive(Error::InvalidConfig {
                message: "OneDrive requires access_token or refresh_token".into(),
            }));
        }
        let buffer_secs = self
            .token_expiry_buffer_secs
            .unwrap_or(DEFAULT_TOKEN_EXPIRY_BUFFER_SECS);
        if has_access && !has_refresh {
            let token = self.access_token.as_deref().ok_or_else(|| {
                StorageError::OneDrive(Error::InvalidConfig {
                    message: "access_token required".into(),
                })
            })?;
            if let Some(exp) = jwt_expires_at(token) {
                let now = SystemTime::now();
                let buffer = Duration::from_secs(buffer_secs);
                if exp <= now + buffer {
                    return Err(StorageError::OneDrive(Error::InvalidConfig {
                        message: "access_token is expired and no refresh_token provided".into(),
                    }));
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
        .map_err(|source| StorageError::OneDrive(Error::Token { source }))?;
        Ok(OneDriveStorage {
            root: self.root,
            endpoint,
            token: token_source,
            fetch: UreqHttpGet::new(),
        })
    }
}

pub struct OneDriveStorage<T = OneDriveTokenSource, F = UreqHttpGet>
where
    T: BearerToken,
    F: HttpGet,
{
    root: PathBuf,
    endpoint: String,
    token: T,
    fetch: F,
}

impl<T, F> OneDriveStorage<T, F>
where
    T: BearerToken,
    F: HttpGet,
{
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

#[derive(Debug)]
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

impl<T, F> Storage for OneDriveStorage<T, F>
where
    T: BearerToken,
    F: HttpGet,
{
    type Entry = OneDriveDirEntry;
    type Iter = std::vec::IntoIter<OneDriveDirEntry>;

    fn read_dir(&self, path: PathBuf) -> StorageResult<Self::Iter> {
        let token = self.token.access_token().map_err(StorageError::OneDrive)?;
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
        let auth = format!("Bearer {}", token);
        let headers = [("Authorization", auth.as_str())];
        let (status, body) = self
            .fetch
            .get(&url, &headers)
            .map_err(StorageError::OneDrive)?;
        if status != 200 {
            let text = String::from_utf8_lossy(&body).into_owned();
            return Err(StorageError::OneDrive(Error::ListFailed {
                url,
                status,
                body: text,
            }));
        }
        let parsed: GraphChildrenResponse = serde_json::from_slice(&body).map_err(|source| {
            StorageError::OneDrive(Error::ListResponse {
                url: url.clone(),
                source,
            })
        })?;
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
    ) -> StorageResult<()> {
        let token = self.token.access_token().map_err(StorageError::OneDrive)?;
        let full_path = self.root.join(path);
        let segment = Self::path_to_graph_segment(&full_path);
        let url = format!("{}/me/drive/root:{}:/content", self.endpoint, segment);
        let range_header = format!("bytes={}-{}", range.start, range.end.saturating_sub(1));
        let auth = format!("Bearer {}", token);
        let headers = [
            ("Authorization", auth.as_str()),
            ("Range", range_header.as_str()),
        ];
        let (status, body) = self
            .fetch
            .get(&url, &headers)
            .map_err(StorageError::OneDrive)?;
        if status != 200 && status != 206 {
            let text = String::from_utf8_lossy(&body).into_owned();
            return Err(StorageError::OneDrive(Error::DownloadFailed {
                url,
                status,
                body: text,
            }));
        }
        let mut pos = range.start;
        let mut remaining = range.end - range.start;
        let mut offset = 0;
        while remaining > 0 && offset < body.len() {
            let take = remaining.min((body.len() - offset) as u64) as usize;
            writer
                .write_at(&body[offset..offset + take], pos)
                .map_err(|error| StorageError::WriteAt {
                    offset: pos,
                    message: error.to_string(),
                })?;
            pos += take as u64;
            remaining -= take as u64;
            offset += take;
        }
        Ok(())
    }
}

pub(crate) fn parse_last_modified(s: &str) -> SystemTime {
    DateTime::parse_from_rfc3339(s.trim())
        .map(|dt| dt.with_timezone(&Utc).into())
        .unwrap_or(UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    type DefaultStorage = OneDriveStorage<OneDriveTokenSource, UreqHttpGet>;

    fn storage_with_response(status: u16, body: &[u8]) -> OneDriveStorage<StubToken, MockHttpGet> {
        OneDriveStorage::<StubToken, MockHttpGet> {
            root: PathBuf::from("/"),
            endpoint: "https://example.com".into(),
            token: StubToken("test".into()),
            fetch: MockHttpGet {
                status,
                body: body.to_vec(),
            },
        }
    }

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
    fn config_fails_with_no_token_returns_invalid_config_error() {
        let config = OneDriveConfig {
            root: PathBuf::from("/"),
            endpoint: "https://graph.microsoft.com/v1.0".into(),
            access_token: None,
            refresh_token: None,
            client_id: None,
            token_expiry_buffer_secs: None,
        };

        let result = config.connect();
        let Err(err) = result else {
            panic!("config should fail")
        };
        assert!(matches!(
            err,
            crate::storages::Error::OneDrive(Error::InvalidConfig { .. })
        ));
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
            DefaultStorage::path_to_graph_segment(&PathBuf::from("")),
            ""
        );
        assert_eq!(
            DefaultStorage::path_to_graph_segment(&PathBuf::from(".")),
            ""
        );
        assert_eq!(
            DefaultStorage::path_to_graph_segment(&PathBuf::from("/.")),
            ""
        );
        assert_eq!(
            DefaultStorage::path_to_graph_segment(&PathBuf::from("Docs")),
            "/Docs"
        );
        assert!(DefaultStorage::path_to_graph_segment(&PathBuf::from("a/b")).contains("a"));
        assert!(
            DefaultStorage::path_to_graph_segment(&PathBuf::from("Docs\\File.pdf"))
                .contains("Docs")
        );
        assert!(
            DefaultStorage::path_to_graph_segment(&PathBuf::from("La Desertica.pdf"))
                .contains("%20")
        );
        assert!(DefaultStorage::path_to_graph_segment(&PathBuf::from("a/b/c")).contains("a"));
    }

    #[test]
    fn parse_last_modified_valid() {
        let t = parse_last_modified("2024-01-01T00:00:00Z");
        assert!(t > UNIX_EPOCH);
    }

    #[test]
    fn parse_last_modified_invalid() {
        let t = parse_last_modified("not-a-date");
        assert_eq!(t, UNIX_EPOCH);
    }

    #[test]
    fn parse_last_modified_empty() {
        let t = parse_last_modified("");
        assert_eq!(t, UNIX_EPOCH);
    }

    struct StubToken(String);

    impl BearerToken for StubToken {
        fn access_token(&self) -> Result<String> {
            Ok(self.0.clone())
        }
    }

    struct MockHttpGet {
        status: u16,
        body: Vec<u8>,
    }

    impl HttpGet for MockHttpGet {
        fn get(&self, _url: &str, _headers: &[(&str, &str)]) -> Result<(u16, Vec<u8>)> {
            Ok((self.status, self.body.clone()))
        }
    }

    struct RecordingWriter {
        writes: Vec<(u64, Vec<u8>)>,
    }

    impl RecordingWriter {
        fn new() -> Self {
            Self { writes: Vec::new() }
        }
        fn total_bytes(&self) -> u64 {
            self.writes.iter().map(|(_, b)| b.len() as u64).sum()
        }
    }

    impl WriteAt for RecordingWriter {
        fn write_at(&mut self, buf: &[u8], offset: u64) -> StorageResult<()> {
            self.writes.push((offset, buf.to_vec()));
            Ok(())
        }
    }

    #[test]
    fn read_dir_returns_entry_from_mock() {
        let list_json = br#"{"value":[{"name":"f.txt","size":100,"lastModifiedDateTime":"2024-01-01T00:00:00Z"}]}"#;
        let storage = storage_with_response(200, list_json);
        let iter = storage.read_dir(PathBuf::new()).unwrap();
        let entries: Vec<_> = iter.collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name(), "f.txt");
        assert_eq!(entries[0].size(), 100);
        assert!(!entries[0].is_dir());
    }

    #[test]
    fn read_dir_http_failure_returns_list_error() {
        let storage = storage_with_response(500, b"boom");
        let result = storage.read_dir(PathBuf::new());
        let Err(err) = result else {
            panic!("list should fail")
        };

        assert!(matches!(
            err,
            crate::storages::Error::OneDrive(Error::ListFailed { status: 500, .. })
        ));
    }

    #[test]
    fn read_file_at_writes_exact_range_from_mock() {
        let body: Vec<u8> = (0..5000).map(|i| (i % 256) as u8).collect();
        let storage = OneDriveStorage::<StubToken, MockHttpGet> {
            root: PathBuf::from("/"),
            endpoint: "https://example.com".into(),
            token: StubToken("test".into()),
            fetch: MockHttpGet {
                status: 206,
                body: body.clone(),
            },
        };
        let mut writer = RecordingWriter::new();
        storage
            .read_file_at(PathBuf::from("f"), &mut writer, 0..5000)
            .unwrap();
        assert_eq!(writer.total_bytes(), 5000);
        let flat: Vec<u8> = writer
            .writes
            .iter()
            .flat_map(|(_, b)| b.iter().copied())
            .collect();
        assert_eq!(flat, body);
    }

    #[test]
    fn read_file_at_caps_at_range_when_mock_returns_more() {
        let body: Vec<u8> = (0..10_000).map(|i| (i % 256) as u8).collect();
        let storage = OneDriveStorage::<StubToken, MockHttpGet> {
            root: PathBuf::from("/"),
            endpoint: "https://example.com".into(),
            token: StubToken("test".into()),
            fetch: MockHttpGet { status: 206, body },
        };
        let mut writer = RecordingWriter::new();
        storage
            .read_file_at(PathBuf::from("f"), &mut writer, 0..5000)
            .unwrap();
        assert_eq!(writer.total_bytes(), 5000);
    }
}
