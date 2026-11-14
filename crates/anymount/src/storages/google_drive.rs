//! Implements Google Drive storage.

use std::any::Any;
use std::ops::Range;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::domain::{
    ConnectStorageError, DirEntry, MAX_READ_SIZE, ReadDirError,
    ReadFileAtError, Storage, StorageConfig, StorageEntry, StoragePath,
    WriteAt, WriteFileError,
};

const DEFAULT_ENDPOINT: &str = "https://www.googleapis.com/drive/v3/";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";

fn default_endpoint() -> String {
    DEFAULT_ENDPOINT.into()
}

fn default_root() -> String {
    "root".into()
}

/// Google Drive configuration using an externally obtained access token.
///
/// The token must grant access to the desired files and must be renewed
/// externally when it expires. `root_id` defaults to the user's Drive root.
#[derive(Clone, Deserialize, Serialize)]
pub struct GoogleDriveConfig {
    pub access_token: Option<String>,
    #[serde(default = "default_root")]
    pub root_id: String,
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
}

#[typetag::serde(name = "gdrive")]
#[async_trait::async_trait(?Send)]
impl StorageConfig for GoogleDriveConfig {
    fn clone_config(&self) -> Box<dyn StorageConfig> {
        Box::new(self.clone())
    }

    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        let token =
            self.access_token.as_ref().filter(|v| !v.is_empty()).ok_or(
                ConnectStorageError::NotAuthenticated { kind: "gdrive" },
            )?;
        let mut endpoint = Url::parse(&self.endpoint).map_err(|e| {
            ConnectStorageError::Failed {
                kind: "gdrive",
                message: e.to_string(),
            }
        })?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.cannot_be_a_base()
        {
            return Err(ConnectStorageError::Failed {
                kind: "gdrive",
                message: "invalid API endpoint".into(),
            });
        }
        if !endpoint.path().ends_with('/') {
            endpoint.set_path(&format!("{}/", endpoint.path()));
        }
        if self.root_id.is_empty() {
            return Err(ConnectStorageError::Failed {
                kind: "gdrive",
                message: "root_id cannot be empty".into(),
            });
        }
        Ok(Box::new(GoogleDriveStorage {
            root_id: self.root_id.clone(),
            endpoint,
            token: token.clone(),
            http: Arc::new(UreqHttp),
        }))
    }
}

/// Sends requests to the Drive API.
pub trait Http: Send + Sync + 'static {
    fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, Vec<u8>), String>;
}

/// Sends Drive requests using ureq.
pub struct UreqHttp;

impl Http for UreqHttp {
    fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, Vec<u8>), String> {
        let mut request = ureq::request(method, url);
        for (name, value) in headers {
            request = request.set(name, value);
        }
        let result = if method == "GET" {
            request.call()
        } else {
            request.send_bytes(body)
        };
        let response = match result {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(error) => return Err(error.to_string()),
        };
        let status = response.status();
        let range = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("range"))
            .map(|(_, value)| *value);
        let content_range = response.header("Content-Range").map(str::to_owned);
        let mut bytes = Vec::new();
        let limit = if range.is_some() {
            MAX_READ_SIZE as u64 + 1
        } else {
            u64::MAX
        };
        std::io::Read::read_to_end(
            &mut std::io::Read::take(response.into_reader(), limit),
            &mut bytes,
        )
        .map_err(|e| e.to_string())?;
        if let Some(requested) = range {
            if status == 206 {
                let (start, end) = requested
                    .strip_prefix("bytes=")
                    .and_then(|r| r.split_once('-'))
                    .and_then(|(a, b)| {
                        Some((a.parse::<u64>().ok()?, b.parse::<u64>().ok()?))
                    })
                    .ok_or("invalid requested range")?;
                let (actual_start, actual_end) = content_range
                    .as_deref()
                    .and_then(|r| r.strip_prefix("bytes "))
                    .and_then(|r| r.split_once('/'))
                    .and_then(|(r, _)| r.split_once('-'))
                    .and_then(|(a, b)| {
                        Some((a.parse::<u64>().ok()?, b.parse::<u64>().ok()?))
                    })
                    .ok_or("missing or invalid Content-Range")?;
                if actual_start != start
                    || actual_end < actual_start
                    || actual_end > end
                    || actual_end - actual_start + 1 != bytes.len() as u64
                {
                    return Err("mismatched Content-Range".into());
                }
            }
        }

        Ok((status, bytes))
    }
}

/// Reads files from a Google Drive folder.
pub struct GoogleDriveStorage<F = UreqHttp> {
    root_id: String,
    endpoint: Url,
    token: String,
    http: Arc<F>,
}

impl<F> Clone for GoogleDriveStorage<F> {
    fn clone(&self) -> Self {
        Self {
            root_id: self.root_id.clone(),
            endpoint: self.endpoint.clone(),
            token: self.token.clone(),
            http: self.http.clone(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct File {
    id: String,
    name: String,
    mime_type: String,
    size: Option<String>,
    viewed_by_me_time: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileList {
    #[serde(default)]
    files: Vec<File>,
    next_page_token: Option<String>,
    #[serde(default)]
    incomplete_search: bool,
}

#[derive(Debug)]
enum Failure {
    Status(u16, String),
    Other(String),
    Missing,
    Ambiguous,
    NotDirectory,
    NotFile,
    Unsupported,
}

fn quote_query(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

impl<F: Http> GoogleDriveStorage<F> {
    fn get(&self, url: &Url, range: Option<&str>) -> Result<Vec<u8>, Failure> {
        let authorization = format!("Bearer {}", self.token);
        let mut headers = vec![("Authorization", authorization.as_str())];
        if let Some(range) = range {
            headers.push(("Range", range));
        }
        let (status, body) = self
            .http
            .request("GET", url.as_str(), &headers, &[])
            .map_err(|e| Failure::Other(e.to_string()))?;
        if range.is_some_and(|value| {
            status == 200 && !value.starts_with("bytes=0-")
        }) {
            return Err(Failure::Other(
                "server ignored the range request".into(),
            ));
        }
        if status != 200 && status != 206 {
            return Err(Failure::Status(
                status,
                String::from_utf8_lossy(&body).into_owned(),
            ));
        }
        Ok(body)
    }

    fn list(
        &self,
        parent: &str,
        name: Option<&str>,
    ) -> Result<Vec<File>, Failure> {
        let mut query =
            format!("'{}' in parents and trashed = false", quote_query(parent));
        if let Some(name) = name {
            query.push_str(&format!(" and name = '{}'", quote_query(name)));
        }
        let mut files = Vec::new();
        let mut page: Option<String> = None;
        let mut seen = std::collections::HashSet::new();
        loop {
            let mut url = self
                .endpoint
                .join("files")
                .map_err(|e| Failure::Other(e.to_string()))?;
            {
                let mut params = url.query_pairs_mut();
                params.append_pair("q", &query);
                params.append_pair(
                    "fields",
                    "nextPageToken,incompleteSearch,files(id,name,mimeType,size,viewedByMeTime)",
                );
                params.append_pair("pageSize", "1000");
                if let Some(token) = &page {
                    params.append_pair("pageToken", token);
                }
            }
            let body = self.get(&url, None)?;
            let response: FileList = serde_json::from_slice(&body)
                .map_err(|e| Failure::Other(e.to_string()))?;
            if response.incomplete_search {
                return Err(Failure::Other(
                    "Drive returned an incomplete listing".into(),
                ));
            }
            files.extend(response.files);
            match response.next_page_token {
                Some(next) if !next.is_empty() && seen.insert(next.clone()) => {
                    page = Some(next)
                }
                Some(_) => {
                    return Err(Failure::Other(
                        "invalid pagination token".into(),
                    ));
                }
                None => break,
            }
        }
        Ok(files)
    }

    fn resolve(&self, path: &StoragePath) -> Result<Option<File>, Failure> {
        let mut parent = self.root_id.clone();
        let mut found = None;
        for component in path.as_path().components() {
            if let Some(file) = &found {
                let file: &File = file;
                if file.mime_type != FOLDER_MIME {
                    return Err(Failure::NotDirectory);
                }
            }
            let name = component
                .as_os_str()
                .to_str()
                .ok_or_else(|| Failure::Other("path is not UTF-8".into()))?;
            let mut matches = self.list(&parent, Some(name))?;
            if matches.len() > 1 {
                return Err(Failure::Ambiguous);
            }
            let file = matches.pop().ok_or(Failure::Missing)?;
            parent = file.id.clone();
            found = Some(file);
        }
        Ok(found)
    }

    fn read_dir_sync(
        &self,
        path: StoragePath,
    ) -> Result<Vec<Box<dyn DirEntry>>, Failure> {
        let parent = match self.resolve(&path)? {
            Some(file) if file.mime_type == FOLDER_MIME => file.id,
            Some(_) => return Err(Failure::NotDirectory),
            None => self.root_id.clone(),
        };
        let files = self.list(&parent, None)?;
        Ok(files
            .into_iter()
            .map(|file| {
                let accessed = file
                    .viewed_by_me_time
                    .as_deref()
                    .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
                    .map(Into::into);
                Box::new(StorageEntry {
                    name: file.name,
                    is_directory: file.mime_type == FOLDER_MIME,
                    size: file.size.and_then(|v| v.parse().ok()),
                    accessed,
                }) as Box<dyn DirEntry>
            })
            .collect())
    }

    fn write_sync(
        &self,
        path: StoragePath,
        data: &[u8],
    ) -> Result<(), Failure> {
        let name = path
            .as_path()
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(Failure::NotFile)?;
        let parent_path =
            path.as_path().parent().ok_or(Failure::NotDirectory)?;
        let parent_path = StoragePath::try_from(parent_path.to_path_buf())
            .map_err(|e| Failure::Other(e.to_string()))?;
        let parent = match self.resolve(&parent_path)? {
            None => self.root_id.clone(),
            Some(file) if file.mime_type == FOLDER_MIME => file.id,
            Some(_) => return Err(Failure::NotDirectory),
        };
        let mut found = self.list(&parent, Some(name))?;
        if found.len() > 1 {
            return Err(Failure::Ambiguous);
        }
        let mut url = self.endpoint.clone();
        url.set_path(&format!("/upload{}", self.endpoint.path()));
        let (method, content_type, body) = if let Some(file) = found.pop() {
            if file.mime_type == FOLDER_MIME {
                return Err(Failure::NotFile);
            }
            if file.mime_type.starts_with("application/vnd.google-apps.") {
                return Err(Failure::Unsupported);
            }
            url.path_segments_mut()
                .map_err(|_| Failure::Other("invalid API endpoint".into()))?
                .pop_if_empty()
                .push("files")
                .push(&file.id);
            url.query_pairs_mut().append_pair("uploadType", "media");
            (
                "PATCH",
                "application/octet-stream".to_string(),
                data.to_vec(),
            )
        } else {
            url.path_segments_mut()
                .map_err(|_| Failure::Other("invalid API endpoint".into()))?
                .pop_if_empty()
                .push("files");
            url.query_pairs_mut().append_pair("uploadType", "multipart");
            let mut boundary = "anymount-gdrive-boundary".to_string();
            let metadata =
                serde_json::json!({ "name": name, "parents": [parent] })
                    .to_string();
            while metadata.contains(&boundary)
                || data
                    .windows(boundary.len())
                    .any(|window| window == boundary.as_bytes())
            {
                boundary.push('x');
            }
            let mut body = format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n--{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
            body.extend_from_slice(data);
            body.extend_from_slice(
                format!("\r\n--{boundary}--\r\n").as_bytes(),
            );
            (
                "POST",
                format!("multipart/related; boundary={boundary}"),
                body,
            )
        };
        let authorization = format!("Bearer {}", self.token);
        let (status, response) = self
            .http
            .request(
                method,
                url.as_str(),
                &[
                    ("Authorization", &authorization),
                    ("Content-Type", &content_type),
                ],
                &body,
            )
            .map_err(Failure::Other)?;
        if !(200..300).contains(&status) {
            return Err(Failure::Status(
                status,
                String::from_utf8_lossy(&response).into_owned(),
            ));
        }
        Ok(())
    }

    fn read_chunk(
        &self,
        id: &str,
        range: Range<u64>,
    ) -> Result<Vec<u8>, Failure> {
        let mut url = self.endpoint.clone();
        url.path_segments_mut()
            .map_err(|_| Failure::Other("invalid API endpoint".into()))?
            .pop_if_empty()
            .push("files")
            .push(id);
        url.query_pairs_mut().append_pair("alt", "media");
        let header = format!("bytes={}-{}", range.start, range.end - 1);
        let bytes = self.get(&url, Some(&header))?;
        if bytes.len() > (range.end - range.start) as usize {
            return Err(Failure::Other(
                "server returned too many bytes".into(),
            ));
        }
        Ok(bytes)
    }
}

fn dir_error(error: Failure) -> ReadDirError {
    match error {
        Failure::Missing | Failure::Status(404, _) => ReadDirError::NotFound,
        Failure::NotDirectory => ReadDirError::NotADirectory,
        Failure::Status(401 | 403, _) => ReadDirError::PermissionDenied,
        Failure::Status(429 | 500..=599, message) => {
            ReadDirError::Unavailable { message }
        }
        Failure::Other(message) => ReadDirError::Unknown { message },
        other => ReadDirError::Unknown {
            message: format!("{other:?}"),
        },
    }
}
fn file_error(error: Failure) -> ReadFileAtError {
    match error {
        Failure::Missing | Failure::Status(404, _) => ReadFileAtError::NotFound,
        Failure::NotFile | Failure::NotDirectory => ReadFileAtError::NotAFile,
        Failure::Status(401 | 403, _) => ReadFileAtError::PermissionDenied,
        Failure::Status(416, _) => ReadFileAtError::RangeNotSatisfiable,
        Failure::Status(429 | 500..=599, message) => {
            ReadFileAtError::Unavailable { message }
        }
        Failure::Other(message) => ReadFileAtError::Unknown { message },
        other => ReadFileAtError::Unknown {
            message: format!("{other:?}"),
        },
    }
}

#[async_trait::async_trait(?Send)]
impl<F: Http> Storage for GoogleDriveStorage<F> {
    async fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<Vec<Box<dyn DirEntry>>, ReadDirError> {
        let storage = self.clone();
        tokio::task::spawn_blocking(move || {
            storage.read_dir_sync(path).map_err(dir_error)
        })
        .await
        .map_err(|e| ReadDirError::Unknown {
            message: e.to_string(),
        })?
    }

    async fn read_file_at(
        &self,
        path: StoragePath,
        writer: &mut dyn WriteAt,
        range: Range<u64>,
    ) -> Result<u64, ReadFileAtError> {
        if range.start > range.end {
            return Err(ReadFileAtError::Unknown {
                message: "invalid range".into(),
            });
        }
        if range.start == range.end {
            return Ok(0);
        }
        let storage = self.clone();
        let file = tokio::task::spawn_blocking(move || {
            storage.resolve(&path).map_err(file_error)
        })
        .await
        .map_err(|e| ReadFileAtError::Unknown {
            message: e.to_string(),
        })??
        .ok_or(ReadFileAtError::NotAFile)?;
        if file.mime_type == FOLDER_MIME {
            return Err(file_error(Failure::NotFile));
        }
        if file.mime_type.starts_with("application/vnd.google-apps.") {
            return Err(ReadFileAtError::NotSupported);
        }
        let mut offset = range.start;
        while offset < range.end {
            let end = offset + (range.end - offset).min(MAX_READ_SIZE as u64);
            let storage = self.clone();
            let id = file.id.clone();
            let result = tokio::task::spawn_blocking(move || {
                storage.read_chunk(&id, offset..end).map_err(file_error)
            })
            .await
            .map_err(|e| ReadFileAtError::Unknown {
                message: e.to_string(),
            })?;
            let bytes = match result {
                Err(ReadFileAtError::RangeNotSatisfiable) => break,
                other => other?,
            };
            if bytes.is_empty() {
                break;
            }
            let expected = (end - offset) as usize;
            writer.write_at(&bytes, offset)?;
            offset += bytes.len() as u64;
            if bytes.len() < expected {
                break;
            }
        }
        Ok(offset - range.start)
    }

    async fn write_file(
        &self,
        path: StoragePath,
        data: &[u8],
    ) -> Result<(), WriteFileError> {
        let storage = self.clone();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || storage.write_sync(path, &data))
            .await
            .map_err(|e| WriteFileError::Failed {
                message: e.to_string(),
            })?
            .map_err(|error| match error {
                Failure::Missing | Failure::Status(404, _) => {
                    WriteFileError::NotFound
                }
                Failure::NotDirectory => WriteFileError::NotADirectory,
                Failure::NotFile => WriteFileError::NotAFile,
                Failure::Unsupported => WriteFileError::NotSupported,
                Failure::Status(401 | 403, _) => {
                    WriteFileError::PermissionDenied
                }
                Failure::Status(429 | 500..=599, message) => {
                    WriteFileError::Unavailable { message }
                }
                Failure::Other(message) => WriteFileError::Failed { message },
                other => WriteFileError::Failed {
                    message: format!("{other:?}"),
                },
            })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use crate::storages::mock_drive_http::MockDriveHttp;
    fn storage(responses: &[&str]) -> GoogleDriveStorage<MockDriveHttp> {
        GoogleDriveStorage {
            root_id: "root".into(),
            endpoint: Url::parse("https://example.test/drive/v3/")
                .expect("url"),
            token: "token".into(),
            http: Arc::new(MockDriveHttp {
                calls: Mutex::new(Vec::new()),
                writes: Mutex::new(Vec::new()),
                responses: Mutex::new(
                    responses
                        .iter()
                        .map(|r| (200, r.as_bytes().to_vec()))
                        .collect(),
                ),
            }),
        }
    }
    #[test]
    fn queries_escape_drive_literals() {
        assert_eq!(quote_query("a'\\b"), "a\\'\\\\b");
    }
    #[test]
    fn lists_all_pages_and_encodes_query() {
        let storage = storage(&[
            r#"{"files":[{"id":"one","name":"a","mimeType":"text/plain","size":"7"}],"nextPageToken":"next"}"#,
            r#"{"files":[{"id":"two","name":"b","mimeType":"application/vnd.google-apps.folder"}]}"#,
        ]);
        let files = storage.list("root", Some("a' b")).expect("list");
        assert_eq!(files.len(), 2);
        let calls = storage.http.calls.lock().expect("lock");
        let query: Vec<_> = Url::parse(&calls[0])
            .expect("url")
            .query_pairs()
            .into_owned()
            .collect();
        assert!(
            query
                .iter()
                .any(|(k, v)| k == "q" && v.contains("name = 'a\\' b'"))
        );
        assert!(calls[1].contains("pageToken=next"));
    }
    #[test]
    fn rejects_incomplete_and_repeated_listing_pages() {
        let incomplete = storage(&[r#"{"incompleteSearch":true,"files":[]}"#]);
        assert!(matches!(
            incomplete.list("root", None),
            Err(Failure::Other(_))
        ));
        let repeated = storage(&[
            r#"{"files":[],"nextPageToken":"one"}"#,
            r#"{"files":[],"nextPageToken":"two"}"#,
            r#"{"files":[],"nextPageToken":"one"}"#,
        ]);
        assert!(matches!(
            repeated.list("root", None),
            Err(Failure::Other(_))
        ));
    }

    #[test]
    fn duplicate_names_are_not_resolved_arbitrarily() {
        let storage = storage(&[
            r#"{"files":[{"id":"1","name":"a","mimeType":"text/plain"},{"id":"2","name":"a","mimeType":"text/plain"}]}"#,
        ]);
        let path =
            StoragePath::try_from(std::path::PathBuf::from("a")).expect("path");
        assert!(matches!(storage.resolve(&path), Err(Failure::Ambiguous)));
    }
    #[test]
    fn creates_and_overwrites_files() {
        let create = storage(&[r#"{"files":[]}"#, "{}"]);
        let path =
            StoragePath::try_from(std::path::PathBuf::from("report.txt"))
                .expect("path");
        create.write_sync(path.clone(), b"hello").expect("create");
        let calls = create.http.calls.lock().expect("lock");
        assert!(
            calls[1].contains("/upload/drive/v3/files?uploadType=multipart")
        );
        let writes = create.http.writes.lock().expect("lock");
        assert_eq!(writes[1].0, "POST");
        assert!(String::from_utf8_lossy(&writes[1].1).contains("report.txt"));
        assert!(String::from_utf8_lossy(&writes[1].1).contains("hello"));

        let update = storage(&[
            r#"{"files":[{"id":"file1","name":"report.txt","mimeType":"text/plain"}]}"#,
            "{}",
        ]);
        update.write_sync(path, b"new").expect("update");
        let calls = update.http.calls.lock().expect("lock");
        assert!(
            calls[1].contains("/upload/drive/v3/files/file1?uploadType=media")
        );
        let writes = update.http.writes.lock().expect("lock");
        assert_eq!(writes[1], ("PATCH".to_string(), b"new".to_vec()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reads_a_range_from_a_resolved_file() {
        let storage = storage(&[
            r#"{"files":[{"id":"file1","name":"report.txt","mimeType":"text/plain"}]}"#,
            "bcd",
        ]);
        storage.http.responses.lock().expect("lock")[1].0 = 206;
        let path =
            StoragePath::try_from(std::path::PathBuf::from("report.txt"))
                .expect("path");
        let mut writer = crate::test_utils::mocks::BytesWriter(Vec::new());
        assert_eq!(
            Storage::read_file_at(&storage, path, &mut writer, 1..4)
                .await
                .expect("read"),
            3
        );
        assert_eq!(writer.0, b"bcd");
    }

    async fn http_storage(server: &wiremock::MockServer) -> Box<dyn Storage> {
        GoogleDriveConfig {
            access_token: Some("test-token".into()),
            root_id: "root".into(),
            endpoint: format!("{}/drive/v3/", server.uri()),
        }
        .connect()
        .await
        .expect("connect")
    }

    #[tokio::test]
    async fn http_lists_all_pages_with_credentials() {
        use wiremock::matchers::{
            header, method, path, query_param, query_param_is_missing,
        };
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let query = "'root' in parents and trashed = false";
        let fields = "nextPageToken,incompleteSearch,files(id,name,mimeType,size,viewedByMeTime)";
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(header("authorization", "Bearer test-token"))
            .and(query_param("q", query))
            .and(query_param("fields", fields))
            .and(query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "files": [{"id": "folder", "name": "folder", "mimeType": FOLDER_MIME}],
                "nextPageToken": "next"
            })))
            .expect(1)
            .mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(header("authorization", "Bearer test-token"))
            .and(query_param("q", query))
            .and(query_param("pageToken", "next"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "files": [{"id": "file", "name": "report.txt", "mimeType": "text/plain", "size": "6"}]
            })))
            .expect(1)
            .mount(&server).await;
        let entries = http_storage(&server)
            .await
            .read_dir(StoragePath::root())
            .await
            .expect("list");
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_dir());
        assert_eq!(entries[1].file_name(), "report.txt");
        assert_eq!(entries[1].size(), Some(6));
    }

    #[tokio::test]
    async fn http_reads_ranges_and_rejects_bad_content_range() {
        use wiremock::matchers::{header, method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("q", "'root' in parents and trashed = false and name = 'report.txt'"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "files": [{"id": "file1", "name": "report.txt", "mimeType": "text/plain"}]
            })))
            .expect(2).mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/file1"))
            .and(query_param("alt", "media"))
            .and(header("authorization", "Bearer test-token"))
            .and(header("range", "bytes=1-3"))
            .respond_with(
                ResponseTemplate::new(206)
                    .insert_header("Content-Range", "bytes 1-3/6")
                    .set_body_raw("bcd", "application/octet-stream"),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/file1"))
            .and(header("range", "bytes=2-4"))
            .respond_with(
                ResponseTemplate::new(206)
                    .insert_header("Content-Range", "bytes 1-3/6")
                    .set_body_raw("bcd", "application/octet-stream"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let storage = http_storage(&server).await;
        let path =
            StoragePath::try_from(std::path::PathBuf::from("report.txt"))
                .expect("path");
        let mut writer = crate::test_utils::mocks::BytesWriter(Vec::new());
        assert_eq!(
            storage
                .read_file_at(path.clone(), &mut writer, 1..4)
                .await
                .expect("read"),
            3
        );
        assert_eq!(writer.0, b"bcd");
        assert!(matches!(
            storage.read_file_at(path, &mut writer, 2..5).await,
            Err(ReadFileAtError::Unknown { .. })
        ));
        assert_eq!(writer.0, b"bcd");
    }

    #[tokio::test]
    async fn http_creates_and_replaces_file_content() {
        use wiremock::matchers::{
            body_bytes, body_string_contains, header, header_regex, method,
            path, query_param,
        };
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let query =
            "'root' in parents and trashed = false and name = 'new.txt'";
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("q", query))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"files": []})),
            )
            .expect(1)
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/drive/v3/files"))
            .and(query_param("q", query))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "files": [{"id": "file1", "name": "new.txt", "mimeType": "text/plain"}]
            })))
            .expect(1).mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/upload/drive/v3/files"))
            .and(query_param("uploadType", "multipart"))
            .and(header("authorization", "Bearer test-token"))
            .and(header_regex(
                "content-type",
                "^multipart/related; boundary=",
            ))
            .and(body_string_contains("\"name\":\"new.txt\""))
            .and(body_string_contains("\"parents\":[\"root\"]"))
            .and(body_string_contains("first content"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/upload/drive/v3/files/file1"))
            .and(query_param("uploadType", "media"))
            .and(header("authorization", "Bearer test-token"))
            .and(header("content-type", "application/octet-stream"))
            .and(body_bytes(b"replacement".to_vec()))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let storage = http_storage(&server).await;
        let path = StoragePath::try_from(std::path::PathBuf::from("new.txt"))
            .expect("path");
        storage
            .write_file(path.clone(), b"first content")
            .await
            .expect("create");
        storage
            .write_file(path, b"replacement")
            .await
            .expect("replace");
    }

    #[tokio::test]
    async fn http_reports_incomplete_listing_and_permission_errors() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/drive/v3/files"))
            .and(query_param("fields", "nextPageToken,incompleteSearch,files(id,name,mimeType,size,viewedByMeTime)"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"files": [], "incompleteSearch": true})))
            .expect(1).up_to_n_times(1).mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;
        let storage = http_storage(&server).await;
        assert!(matches!(
            storage.read_dir(StoragePath::root()).await,
            Err(ReadDirError::Unknown { .. })
        ));
        assert!(matches!(
            storage.read_dir(StoragePath::root()).await,
            Err(ReadDirError::PermissionDenied)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn config_requires_access_token() {
        let config = GoogleDriveConfig {
            access_token: None,
            root_id: default_root(),
            endpoint: default_endpoint(),
        };
        assert!(matches!(
            config.connect().await,
            Err(ConnectStorageError::NotAuthenticated { .. })
        ));
    }
}
