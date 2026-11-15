//! Exposes shared OneDrive authorization and storage to JavaScript hosts.

use crate::domain::{
    AuthorizationError, ConfigurationPersistence, CredentialUpdate,
    HostSaveError, ReadDirError, ReadFileAtError, SaveStatus, Storage,
    StoragePath, WriteAt, WriteAtError,
};
use crate::storages::onedrive::{
    OneDriveConfig, OneDriveSession, OneDriveStorage,
};
use futures::StreamExt;
use js_sys::{Function, Promise};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::fmt;
use std::path::PathBuf;
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
use wasm_bindgen_futures::JsFuture;

/// Owns one configuration's live OneDrive session and connected storage.
#[wasm_bindgen]
pub struct OneDriveBrowserApi {
    session: OneDriveSession,
    storage: RefCell<Option<OneDriveStorage>>,
    temporary_storage: JsValue,
}

impl fmt::Debug for OneDriveBrowserApi {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OneDriveBrowserApi")
            .field("connected", &self.storage.borrow().is_some())
            .finish_non_exhaustive()
    }
}

#[wasm_bindgen]
impl OneDriveBrowserApi {
    /// Creates a session from provider configuration JSON and host identity.
    ///
    /// Reuse this instance for one saved record; do not create concurrent
    /// owners of that record in other tabs or application instances.
    ///
    /// # Errors
    ///
    /// Returns `SetupError` for invalid configuration or binding.
    #[wasm_bindgen(constructor)]
    pub fn new(config_json: &str, identity: String) -> Result<Self, JsValue> {
        let config: OneDriveConfig = serde_json::from_str(config_json)
            .map_err(|_| authorization_error(AuthorizationError::Setup))?;
        if identity.is_empty()
            || identity.len() > 256
            || !config
                .client_id
                .as_deref()
                .is_some_and(|id| !id.trim().is_empty())
        {
            return Err(authorization_error(AuthorizationError::Setup));
        }
        let session = OneDriveSession::new(config, identity)
            .map_err(authorization_error)?;
        Ok(Self::from_session(session))
    }

    /// Begins PKCE authorization without navigating or opening UI.
    ///
    /// Saves a sensitive, bounded continuation in this tab's sessionStorage
    /// before returning the URL. Navigate only after this succeeds.
    ///
    /// # Errors
    ///
    /// Returns a named authorization error if setup or request creation fails.
    #[wasm_bindgen(js_name = beginRedirect)]
    pub async fn begin_redirect(
        &self,
        redirect_uri: &str,
    ) -> Result<String, JsValue> {
        let redirect = self
            .session
            .begin_redirect(redirect_uri)
            .await
            .map_err(authorization_error)?;
        if let Err(error) = self.store_continuation(&redirect.continuation) {
            self.session.cancel();
            return Err(error);
        }
        encode(json!({
            "authorization_url": redirect.authorization_url,
            "continuation": redirect.continuation,
            "expires_at": redirect.expires_at,
        }))
    }

    /// Restores a short-lived redirect continuation after page navigation.
    ///
    /// # Errors
    ///
    /// Rejects expired, mismatched, or invalid continuation records.
    #[wasm_bindgen(js_name = resumeRedirect)]
    pub fn resume_redirect(&self, continuation: &str) -> Result<(), JsValue> {
        if self.load_continuation()?.as_deref() != Some(continuation) {
            return Err(authorization_error(
                AuthorizationError::InvalidCallback,
            ));
        }
        self.session
            .resume_redirect(continuation)
            .map_err(authorization_error)
    }

    /// Consumes a matching callback and activates its credentials immediately.
    ///
    /// Validates callback correlation before consuming sessionStorage, then
    /// removes the record before exchange. Saving remains independent.
    ///
    /// # Errors
    ///
    /// Returns a named authorization error for rejected callbacks or exchange
    /// failures; raw callback or provider response data is never included.
    #[wasm_bindgen(js_name = completeRedirect)]
    pub async fn complete_redirect(
        &self,
        callback: &str,
    ) -> Result<(), JsValue> {
        self.session
            .validate_redirect_callback(callback)
            .map_err(authorization_error)?;
        if self.load_continuation()?.is_none() {
            return Err(authorization_error(
                AuthorizationError::InvalidCallback,
            ));
        }
        self.remove_continuation()?;
        clear_callback_url(callback)?;
        self.session
            .complete_redirect(callback)
            .await
            .map_err(authorization_error)
    }

    /// Allows a completed temporary renewal failure to be retried explicitly.
    ///
    /// A lost response may have rotated credentials; this never retries an
    /// unusable grant or an exchange that is still running.
    #[wasm_bindgen(js_name = retryRenewal)]
    pub fn retry_renewal(&self) -> bool {
        self.session.retry_renewal()
    }

    /// Starts device authorization when the provider deployment supports it.
    ///
    /// Browser device-flow CORS support is not guaranteed. Present the returned
    /// verification URI and user code only to the signing-in user.
    ///
    /// # Errors
    ///
    /// Returns a named authorization error for unsupported setup or requests.
    #[wasm_bindgen(js_name = beginDevice)]
    pub async fn begin_device(&self) -> Result<String, JsValue> {
        let device = self
            .session
            .begin_device()
            .await
            .map_err(authorization_error)?;
        encode(json!({
            "verification_uri": device.verification_uri,
            "user_code": device.user_code,
            "verification_uri_complete": device.verification_uri_complete,
            "expires_at": device.expires_at,
        }))
    }

    /// Polls the active device attempt using provider-controlled intervals.
    ///
    /// # Errors
    ///
    /// Returns a named authorization error for decline, expiry, cancellation,
    /// setup, or temporary provider failures.
    #[wasm_bindgen(js_name = pollDevice)]
    pub async fn poll_device(&self) -> Result<(), JsValue> {
        self.session
            .poll_device()
            .await
            .map_err(authorization_error)
    }

    /// Cancels the current sign-in attempt without revoking usable credentials.
    pub fn cancel(&self) -> Result<(), JsValue> {
        self.session.cancel();
        self.remove_continuation()
    }

    /// Connects storage through the existing shared authorization session.
    ///
    /// This never opens UI, requests filesystem permission, or saves config.
    ///
    /// # Errors
    ///
    /// Returns `SignInRequiredError` if no credentials are available, or
    /// `SetupError` if the provider configuration cannot connect.
    pub fn connect(&self) -> Result<(), JsValue> {
        let storage = self.session.connect().map_err(|error| match error {
            crate::domain::ConnectStorageError::NotAuthenticated { .. } => {
                authorization_error(AuthorizationError::SignInRequired)
            }
            _ => authorization_error(AuthorizationError::Setup),
        })?;
        *self.storage.borrow_mut() = Some(storage);
        Ok(())
    }

    /// Disconnects this host handle without cancelling authorization or saves.
    pub fn disconnect(&self) {
        self.storage.borrow_mut().take();
    }

    /// Lists a storage-relative directory as JSON entry records.
    ///
    /// # Errors
    ///
    /// Returns a named storage error for invalid paths or failed access.
    #[wasm_bindgen(js_name = readDir)]
    pub async fn read_dir(&self, path: &str) -> Result<String, JsValue> {
        let storage = self.connected_storage()?;
        let path = storage_path(path)?;
        let mut stream =
            storage.read_dir(path).await.map_err(read_dir_error)?;
        let mut entries = Vec::new();
        while let Some(entry) = stream.next().await {
            let entry = entry.map_err(read_dir_error)?;
            entries.push(json!({
                "name": entry.file_name(),
                "is_dir": entry.is_dir(),
                "size": entry.size(),
            }));
        }
        encode(Value::Array(entries))
    }

    /// Reads a bounded binary range, returning a JavaScript Uint8Array.
    ///
    /// `offset` is a JavaScript bigint. Request at most 65,536 bytes per call;
    /// a short result indicates the end of the file.
    ///
    /// # Errors
    ///
    /// Returns a named storage error for invalid ranges, paths, or failed reads.
    #[wasm_bindgen(js_name = readFile)]
    pub async fn read_file(
        &self,
        path: &str,
        offset: u64,
        length: u32,
    ) -> Result<Vec<u8>, JsValue> {
        let storage = self.connected_storage()?;
        let path = storage_path(path)?;
        if u64::from(length) > crate::domain::MAX_READ_FILE_AT_RANGE_SIZE as u64
        {
            return Err(js_error("RangeError", "file range is too large"));
        }
        let end = offset
            .checked_add(u64::from(length))
            .ok_or_else(|| js_error("RangeError", "invalid file range"))?;
        let mut writer = BrowserReadBuffer(Vec::with_capacity(length as usize));
        storage
            .read_file_at(path, &mut writer, offset..end)
            .await
            .map_err(read_file_error)?;
        Ok(writer.0)
    }

    /// Returns the latest unsaved credential snapshot JSON, or no value.
    ///
    /// The returned config contains secrets. Do not log it or manually
    /// acknowledge it; use `savePending` for serialized, generation-safe saves.
    ///
    /// # Errors
    ///
    /// Returns `HostSaveError` if the snapshot cannot be serialized.
    #[wasm_bindgen(js_name = pendingUpdate)]
    pub fn pending_update(&self) -> Result<Option<String>, JsValue> {
        self.session
            .pending_update()
            .map(|update| update_json(&update).map_err(host_save_error))
            .transpose()
    }

    /// Reports persistence state independently of storage connectivity.
    ///
    /// # Errors
    ///
    /// Returns `HostSaveError` if the state cannot be serialized.
    #[wasm_bindgen(js_name = saveStatus)]
    pub fn save_status(&self) -> Result<String, JsValue> {
        let value = match self.session.save_status() {
            SaveStatus::Clean => json!({"state": "clean"}),
            SaveStatus::Pending { generation } => {
                json!({"state": "pending", "generation": generation})
            }
            SaveStatus::Saved { generation } => {
                json!({"state": "saved", "generation": generation})
            }
            SaveStatus::Failed { generation } => {
                json!({"state": "failed", "generation": generation})
            }
        };
        encode(value)
    }

    /// Saves pending configuration through a host-owned promise callback.
    ///
    /// The callback receives JSON `{generation, config}` and must resolve only
    /// after replacing its saved record while preserving unrelated settings.
    /// Calls serialize through the session and retain newer updates on failure.
    /// Storage access never invokes or waits for this callback.
    /// Once started, saving continues even if its caller stops awaiting it.
    ///
    /// # Errors
    ///
    /// Returns `HostSaveError` on callback throws, promise rejection, or encoding
    /// failure. Callback rejection details are deliberately not exposed.
    #[wasm_bindgen(js_name = savePending)]
    pub async fn save_pending(
        &self,
        callback: Function,
    ) -> Result<(), JsValue> {
        let session = self.session.clone();
        let (sender, receiver) = futures::channel::oneshot::channel();
        wasm_bindgen_futures::spawn_local(async move {
            let result = session
                .save_pending(&BrowserConfigurationPersistence { callback })
                .await;
            let _ = sender.send(result);
        });
        receiver
            .await
            .map_err(|_| host_save_error(HostSaveError))?
            .map_err(host_save_error)
    }
}

impl OneDriveBrowserApi {
    fn from_session(session: OneDriveSession) -> Self {
        Self {
            session,
            storage: RefCell::new(None),
            temporary_storage: js_sys::Reflect::get(
                &js_sys::global(),
                &JsValue::from_str("sessionStorage"),
            )
            .unwrap_or(JsValue::UNDEFINED),
        }
    }

    /// Returns the host-retained session for mounting multiple workspaces.
    pub fn session(&self) -> OneDriveSession {
        self.session.clone()
    }

    fn storage_key(&self) -> String {
        format!("anymount.onedrive.redirect:{}", self.session.identity())
    }

    fn storage_call(
        &self,
        name: &str,
        arguments: &[JsValue],
    ) -> Result<JsValue, JsValue> {
        use wasm_bindgen::JsCast;
        let failure = || {
            js_error(
                "ContinuationStorageError",
                "temporary authorization storage failed",
            )
        };
        let function = js_sys::Reflect::get(
            &self.temporary_storage,
            &JsValue::from_str(name),
        )
        .map_err(|_| failure())?
        .dyn_into::<Function>()
        .map_err(|_| failure())?;
        let args = js_sys::Array::new();
        for argument in arguments {
            args.push(argument);
        }
        function
            .apply(&self.temporary_storage, &args)
            .map_err(|_| failure())
    }

    fn store_continuation(&self, value: &str) -> Result<(), JsValue> {
        if value.len() > 8192 {
            return Err(authorization_error(
                AuthorizationError::InvalidCallback,
            ));
        }
        self.storage_call(
            "setItem",
            &[
                JsValue::from_str(&self.storage_key()),
                JsValue::from_str(value),
            ],
        )?;
        Ok(())
    }

    fn load_continuation(&self) -> Result<Option<String>, JsValue> {
        let value = self.storage_call(
            "getItem",
            &[JsValue::from_str(&self.storage_key())],
        )?;
        if value.is_null() {
            return Ok(None);
        }
        let value = value
            .as_string()
            .filter(|value| value.len() <= 8192)
            .ok_or_else(|| {
                authorization_error(AuthorizationError::InvalidCallback)
            })?;
        Ok(Some(value))
    }

    fn remove_continuation(&self) -> Result<(), JsValue> {
        self.storage_call(
            "removeItem",
            &[JsValue::from_str(&self.storage_key())],
        )?;
        Ok(())
    }

    fn connected_storage(&self) -> Result<OneDriveStorage, JsValue> {
        self.storage.borrow().clone().ok_or_else(|| {
            js_error("DisconnectedError", "storage is disconnected")
        })
    }
}

struct BrowserConfigurationPersistence {
    callback: Function,
}

#[async_trait::async_trait(?Send)]
impl ConfigurationPersistence for BrowserConfigurationPersistence {
    async fn save(
        &self,
        update: &CredentialUpdate,
    ) -> Result<(), HostSaveError> {
        let encoded = update_json(update)?;
        let result = self
            .callback
            .call1(&JsValue::UNDEFINED, &JsValue::from_str(&encoded))
            .map_err(|_| HostSaveError)?;
        JsFuture::from(Promise::resolve(&result))
            .await
            .map_err(|_| HostSaveError)?;
        Ok(())
    }
}

struct BrowserReadBuffer(Vec<u8>);

impl WriteAt for BrowserReadBuffer {
    fn write_at(
        &mut self,
        bytes: &[u8],
        _offset: u64,
    ) -> Result<(), WriteAtError> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}

fn update_json(update: &CredentialUpdate) -> Result<String, HostSaveError> {
    let config =
        serde_json::to_value(&update.config).map_err(|_| HostSaveError)?;
    serde_json::to_string(&json!({
        "generation": update.generation,
        "config": config,
    }))
    .map_err(|_| HostSaveError)
}

fn encode(value: Value) -> Result<String, JsValue> {
    serde_json::to_string(&value).map_err(|_| host_save_error(HostSaveError))
}

fn storage_path(path: &str) -> Result<StoragePath, JsValue> {
    StoragePath::try_from(PathBuf::from(path))
        .map_err(|_| js_error("InvalidPathError", "invalid storage path"))
}

fn authorization_error(error: AuthorizationError) -> JsValue {
    let name = match error {
        AuthorizationError::Temporary => "TemporaryError",
        AuthorizationError::Setup => "SetupError",
        AuthorizationError::SignInRequired => "SignInRequiredError",
        AuthorizationError::PermissionDenied => "PermissionDeniedError",
        AuthorizationError::Declined => "DeclinedError",
        AuthorizationError::Expired => "ExpiredError",
        AuthorizationError::Cancelled => "CancelledError",
        AuthorizationError::InvalidCallback => "InvalidCallbackError",
    };
    js_error(name, &error.to_string())
}

fn host_save_error(_error: HostSaveError) -> JsValue {
    js_error("HostSaveError", "configuration could not be saved")
}

fn read_dir_error(error: ReadDirError) -> JsValue {
    match error {
        ReadDirError::Authorization(error) => authorization_error(error),
        ReadDirError::NotFound => {
            js_error("NotFoundError", "directory not found")
        }
        ReadDirError::NotADirectory => {
            js_error("TypeMismatchError", "path is not a directory")
        }
        ReadDirError::PermissionDenied => {
            js_error("PermissionDeniedError", "storage permission denied")
        }
        ReadDirError::Disconnected => {
            js_error("DisconnectedError", "storage is disconnected")
        }
        _ => js_error("StorageError", "directory could not be read"),
    }
}

fn read_file_error(error: ReadFileAtError) -> JsValue {
    match error {
        ReadFileAtError::Authorization(error) => authorization_error(error),
        ReadFileAtError::NotFound => {
            js_error("NotFoundError", "file not found")
        }
        ReadFileAtError::NotAFile => {
            js_error("TypeMismatchError", "path is not a file")
        }
        ReadFileAtError::PermissionDenied => {
            js_error("PermissionDeniedError", "storage permission denied")
        }
        ReadFileAtError::Disconnected => {
            js_error("DisconnectedError", "storage is disconnected")
        }
        ReadFileAtError::RangeNotSatisfiable => {
            js_error("RangeError", "invalid file range")
        }
        _ => js_error("StorageError", "file could not be read"),
    }
}

fn js_error(name: &str, message: &str) -> JsValue {
    let error = js_sys::Error::new(message);
    error.set_name(name);
    error.into()
}

#[cfg(test)]
#[path = "onedrive_browser_api_tests.rs"]
mod tests;

fn clear_callback_url(callback: &str) -> Result<(), JsValue> {
    let global = js_sys::global();
    let location =
        js_sys::Reflect::get(&global, &JsValue::from_str("location")).map_err(
            |_| js_error("SetupError", "callback location unavailable"),
        )?;
    if location.is_null() || location.is_undefined() {
        return Ok(());
    }
    let href = js_sys::Reflect::get(&location, &JsValue::from_str("href"))
        .map_err(|_| js_error("SetupError", "callback location unavailable"))?;
    if href.as_string().as_deref() != Some(callback) {
        return Ok(());
    }
    let mut url = url::Url::parse(callback).map_err(|_| {
        authorization_error(AuthorizationError::InvalidCallback)
    })?;
    url.set_query(None);
    url.set_fragment(None);
    let history = js_sys::Reflect::get(&global, &JsValue::from_str("history"))
        .map_err(|_| js_error("SetupError", "callback history unavailable"))?;
    use wasm_bindgen::JsCast;
    let replace =
        js_sys::Reflect::get(&history, &JsValue::from_str("replaceState"))
            .map_err(|_| {
                js_error("SetupError", "callback history unavailable")
            })?
            .dyn_into::<Function>()
            .map_err(|_| {
                js_error("SetupError", "callback history unavailable")
            })?;
    let state = js_sys::Reflect::get(&history, &JsValue::from_str("state"))
        .map_err(|_| js_error("SetupError", "callback history unavailable"))?;
    replace
        .call3(
            &history,
            &state,
            &JsValue::from_str(""),
            &JsValue::from_str(url.as_str()),
        )
        .map_err(|_| js_error("SetupError", "callback history unavailable"))?;
    Ok(())
}
