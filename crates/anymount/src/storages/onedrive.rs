//! Portable OneDrive storage backed by an explicitly shared OAuth session.
//!
//! Hosts retain one `OneDriveSession` per configuration identity and pass it to
//! connections. Cloning serialized configuration creates an independent owner.
//! Storage never presents UI or saves configuration. OneDrive remains read-only.

mod session;
#[doc(inline)]
pub use session::{OneDriveSession, OneDriveStatus, RequestCredential};

use crate::domain::{
    AuthorizationError, ConnectStorageError, DirEntry, DirStream, ReadDirError,
    ReadFileAtError, Storage, StorageConfig, StoragePath, WriteAt,
};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const ANYMOUNT_AZURE_APP_CLIENT_ID: &str =
    "5970173e-1b75-4317-987d-6849236cc3df";
const DEFAULT_TOKEN_EXPIRY_BUFFER_SECS: u64 = 60;

/// Records the registration and resource to which the provider grant belongs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantBinding {
    pub provider: String,
    pub client_id: String,
    pub authority: String,
    pub resource: String,
}

/// Encodes OneDrive credentials directly in the host's local configuration.
///
/// Access tokens are opaque. Unknown expiry and absent refresh tokens are valid.
/// `client_id = None` retains the legacy built-in native app interpretation.
/// Browser hosts must supply their own correctly registered public SPA client.
#[derive(Clone, Deserialize, Serialize)]
pub struct OneDriveConfig {
    #[serde(default)]
    pub root: PathBuf,
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub client_id: Option<String>,
    pub token_expiry_buffer_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<GrantBinding>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub credential_generation: u64,
}
fn default_endpoint() -> String {
    "https://graph.microsoft.com/v1.0".into()
}
fn is_zero(value: &u64) -> bool {
    *value == 0
}
impl fmt::Debug for OneDriveConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OneDriveConfig")
            .field("credential_generation", &self.credential_generation)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}
impl OneDriveConfig {
    /// Creates an independent connection; hosts sharing identity use a session.
    ///
    /// # Errors
    /// Returns missing credentials or unsupported registration/resource binding.
    pub fn connect(self) -> Result<OneDriveStorage, ConnectStorageError> {
        OneDriveSession::new(self, String::new())
            .map_err(connect_error)?
            .connect()
    }
    fn expected_binding(&self) -> GrantBinding {
        GrantBinding {
            provider: "onedrive".into(),
            client_id: self
                .client_id
                .clone()
                .unwrap_or_else(|| ANYMOUNT_AZURE_APP_CLIENT_ID.into()),
            authority: "https://login.microsoftonline.com/common/oauth2/v2.0"
                .into(),
            resource: self.endpoint.trim_end_matches('/').into(),
        }
    }
    fn validate(&self) -> Result<(), AuthorizationError> {
        let endpoint = self.endpoint.trim_end_matches('/');
        if !matches!(
            endpoint,
            "https://graph.microsoft.com/v1.0"
                | "https://graph.microsoft.com/beta"
        ) {
            return Err(AuthorizationError::Setup);
        }
        if self
            .client_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty())
        {
            return Err(AuthorizationError::Setup);
        }
        if self
            .binding
            .as_ref()
            .is_some_and(|binding| binding != &self.expected_binding())
        {
            return Err(AuthorizationError::Setup);
        }
        Ok(())
    }
}
fn connect_error(error: AuthorizationError) -> ConnectStorageError {
    if error == AuthorizationError::SignInRequired {
        ConnectStorageError::NotAuthenticated { kind: "onedrive" }
    } else {
        ConnectStorageError::Failed {
            kind: "onedrive",
            message: error.to_string(),
        }
    }
}
#[typetag::serde(name = "onedrive")]
#[async_trait::async_trait(?Send)]
impl StorageConfig for OneDriveConfig {
    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        self.clone()
            .connect()
            .map(|storage| Box::new(storage) as Box<dyn Storage>)
    }
}

/// Connects portable resource requests to a host-owned authorization session.
#[derive(Clone, Debug)]
pub struct OneDriveStorage {
    root: PathBuf,
    endpoint: String,
    session: OneDriveSession,
}
impl OneDriveStorage {
    fn new(config: &OneDriveConfig, session: OneDriveSession) -> Self {
        Self {
            root: config.root.clone(),
            endpoint: config.endpoint.trim_end_matches('/').into(),
            session,
        }
    }
    fn item_url(&self, path: &StoragePath) -> String {
        let full = self.root.join(path.as_path());
        let segment = path_to_graph_segment(&full);
        if segment.is_empty() {
            format!("{}/me/drive/root", self.endpoint)
        } else {
            format!("{}/me/drive/root:{}:", self.endpoint, segment)
        }
    }
    async fn graph_get(
        &self,
        url: &str,
    ) -> Result<oauth2::HttpResponse, AuthorizationError> {
        let credential = self.session.request_credentials(None).await?;
        let response =
            self.get(url, Some(credential.token.as_ref()), None).await?;
        if response.status() != 401 {
            return Ok(response);
        }
        // Graph 401 confirms rejection; only safe reads get one coordinated retry.
        let credential = self
            .session
            .request_credentials(Some(credential.generation))
            .await?;
        let response =
            self.get(url, Some(credential.token.as_ref()), None).await?;
        if response.status() == 401 {
            self.session.reject_credentials(credential.generation);
            return Err(AuthorizationError::SignInRequired);
        }
        Ok(response)
    }
    async fn get(
        &self,
        url: &str,
        bearer: Option<&str>,
        range: Option<&str>,
    ) -> Result<oauth2::HttpResponse, AuthorizationError> {
        let mut builder =
            oauth2::http::Request::builder().uri(url).method("GET");
        if let Some(token) = bearer {
            builder =
                builder.header("Authorization", format!("Bearer {token}"));
        }
        if let Some(range) = range {
            builder = builder.header("Range", range);
        }
        let request = builder
            .body(Vec::new())
            .map_err(|_| AuthorizationError::Setup)?;
        self.session.runtime().request(request).await
    }
}
fn path_to_graph_segment(path: &Path) -> String {
    let mut segment = String::new();
    for component in path.components() {
        if let Component::Normal(name) = component {
            segment.push('/');
            segment.push_str(&urlencoding::encode(&name.to_string_lossy()));
        }
    }
    segment
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphDriveItem {
    name: String,
    size: Option<u64>,
    folder: Option<serde_json::Value>,
    last_modified_date_time: Option<String>,
}
#[derive(Deserialize)]
struct GraphChildrenResponse {
    value: Vec<GraphDriveItem>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}
#[derive(Deserialize)]
struct DownloadMetadata {
    #[serde(rename = "@microsoft.graph.downloadUrl")]
    download_url: String,
}

/// Contains OneDrive entry metadata without exposing request credentials.
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
    fn size(&self) -> Option<u64> {
        Some(self.size)
    }
    fn accessed(&self) -> Option<SystemTime> {
        Some(self.accessed)
    }
}
fn read_dir_status(status: u16) -> ReadDirError {
    match status {
        401 => AuthorizationError::SignInRequired.into(),
        403 => ReadDirError::PermissionDenied,
        404 => ReadDirError::NotFound,
        429 | 500..=599 => AuthorizationError::Temporary.into(),
        _ => ReadDirError::Unknown {
            message: "OneDrive directory response failed".into(),
        },
    }
}
fn read_status(status: u16) -> ReadFileAtError {
    match status {
        401 => AuthorizationError::SignInRequired.into(),
        403 => ReadFileAtError::PermissionDenied,
        404 => ReadFileAtError::NotFound,
        416 => ReadFileAtError::RangeNotSatisfiable,
        429 | 500..=599 => AuthorizationError::Temporary.into(),
        _ => ReadFileAtError::Unknown {
            message: "OneDrive download response failed".into(),
        },
    }
}
fn safe_download_url(value: &str) -> Result<(), AuthorizationError> {
    let url = url::Url::parse(value).map_err(|_| AuthorizationError::Setup)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(AuthorizationError::Setup);
    }
    Ok(())
}
#[async_trait::async_trait(?Send)]
impl Storage for OneDriveStorage {
    async fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<DirStream, ReadDirError> {
        let mut url = format!("{}/children", self.item_url(&path));
        let mut entries: Vec<Result<Box<dyn DirEntry>, ReadDirError>> =
            Vec::new();
        loop {
            let response = self.graph_get(&url).await?;
            if response.status() != 200 {
                return Err(read_dir_status(response.status().as_u16()));
            }
            let parsed: GraphChildrenResponse =
                serde_json::from_slice(response.body()).map_err(|_| {
                    ReadDirError::Unknown {
                        message: "invalid OneDrive directory metadata".into(),
                    }
                })?;
            entries.extend(parsed.value.into_iter().map(|item| {
                let accessed = item
                    .last_modified_date_time
                    .as_deref()
                    .and_then(|text| {
                        chrono::DateTime::parse_from_rfc3339(text).ok()
                    })
                    .map(SystemTime::from)
                    .unwrap_or(UNIX_EPOCH);
                Ok(Box::new(OneDriveDirEntry {
                    file_name: item.name,
                    is_dir: item.folder.is_some(),
                    size: item.size.unwrap_or(0),
                    accessed,
                }) as Box<dyn DirEntry>)
            }));
            let Some(next) = parsed.next_link else {
                break;
            };
            // Pagination is provider supplied, but never expands bearer destinations.
            if !next.starts_with(&format!("{}/", self.endpoint)) {
                return Err(AuthorizationError::Setup.into());
            }
            url = next;
        }
        Ok(Box::pin(futures::stream::iter(entries)))
    }
    async fn read_file_at(
        &self,
        path: StoragePath,
        writer: &mut dyn WriteAt,
        range: std::ops::Range<u64>,
    ) -> Result<u64, ReadFileAtError> {
        if range.start > range.end {
            return Err(ReadFileAtError::RangeNotSatisfiable);
        }
        if range.is_empty() {
            return Ok(0);
        }
        // Graph /content redirects authenticated requests and is unsuitable for
        // browsers. Metadata provides a secret preauthenticated download URL.
        let metadata = self.graph_get(&self.item_url(&path)).await?;
        if metadata.status() != 200 {
            return Err(read_status(metadata.status().as_u16()));
        }
        let metadata: DownloadMetadata =
            serde_json::from_slice(metadata.body()).map_err(|_| {
                ReadFileAtError::Unknown {
                    message: "OneDrive download URL unavailable".into(),
                }
            })?;
        safe_download_url(&metadata.download_url)?;
        let mut offset = range.start;
        while offset < range.end {
            let end = offset
                + (range.end - offset)
                    .min(crate::domain::MAX_READ_FILE_AT_RANGE_SIZE as u64);
            let header = format!("bytes={offset}-{}", end - 1);
            // Deliberately never attach bearer credentials to download URLs.
            let response = self
                .get(&metadata.download_url, None, Some(&header))
                .await?;
            let status = response.status().as_u16();
            if status == 416 {
                break;
            }
            if status != 200 && status != 206 {
                return Err(read_status(status));
            }
            if status == 200 && offset != 0 {
                return Err(ReadFileAtError::Unknown {
                    message: "server ignored range request".into(),
                });
            }
            let body = response.body();
            if body.len() > (end - offset) as usize {
                return Err(ReadFileAtError::Unknown {
                    message: "download exceeded requested range".into(),
                });
            }
            if body.is_empty() {
                break;
            }
            writer.write_at(body, offset)?;
            offset += body.len() as u64;
            if body.len() < (end - (offset - body.len() as u64)) as usize {
                break;
            }
        }
        Ok(offset - range.start)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
