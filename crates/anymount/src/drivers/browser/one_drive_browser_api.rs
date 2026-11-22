use crate::domain::{
    AuthorizationError, CredentialUpdate, ReadDirError, ReadFileAtError,
    Storage, StoragePath,
};
use crate::storages::onedrive::{
    OneDriveConfig, OneDriveConnector, OneDriveStorage,
};
use futures::StreamExt;
use js_sys::{Function, Promise};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
pub struct OneDriveBrowserApi {
    connector: OneDriveConnector,
    saves: Rc<BrowserSaves>,
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
    /// Creates a connector from configuration JSON and a stable host identity.
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
        let connector = config
            .into_connector(identity)
            .map_err(authorization_error)?;
        Ok(Self::from_connector(connector))
    }

    /// Begins PKCE authorization without navigating or opening UI.
    ///
    /// Saves a sensitive, bounded continuation in this tab's sessionStorage
    /// before returning the URL. Navigate only after this succeeds.
    /// Storage failure prevents the redirect from being issued.
    #[wasm_bindgen(js_name = beginRedirect)]
    pub async fn begin_redirect(
        &self,
        redirect_uri: &str,
    ) -> Result<String, JsValue> {
        let redirect = self
            .connector
            .begin_redirect(redirect_uri)
            .await
            .map_err(authorization_error)?;
        if let Err(error) = self.store_continuation(&redirect.continuation) {
            self.connector.cancel();
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
        self.connector
            .resume_redirect(continuation)
            .map_err(authorization_error)
    }

    /// Consumes a matching callback and activates its credentials immediately.
    ///
    /// A mismatched callback leaves the current continuation untouched.
    /// After validation the continuation is removed, even if exchange fails.
    #[wasm_bindgen(js_name = completeRedirect)]
    pub async fn complete_redirect(
        &self,
        callback: &str,
    ) -> Result<(), JsValue> {
        self.connector
            .validate_redirect_callback(callback)
            .map_err(authorization_error)?;
        if self.load_continuation()?.is_none() {
            return Err(authorization_error(
                AuthorizationError::InvalidCallback,
            ));
        }
        self.remove_continuation()?;
        clear_callback_url(callback)?;
        self.connector
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
        self.connector.retry_renewal()
    }

    /// Returns device instructions to show only to the signing-in user.
    ///
    /// The provider must allow device authorization from this browser origin.
    #[wasm_bindgen(js_name = beginDevice)]
    pub async fn begin_device(&self) -> Result<String, JsValue> {
        let device = self
            .connector
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

    #[wasm_bindgen(js_name = pollDevice)]
    pub async fn poll_device(&self) -> Result<(), JsValue> {
        self.connector
            .poll_device()
            .await
            .map_err(authorization_error)
    }

    /// Cancels the current sign-in attempt without revoking usable credentials.
    pub fn cancel(&self) -> Result<(), JsValue> {
        self.connector.cancel();
        self.remove_continuation()
    }

    /// Connects without prompting or saving configuration.
    ///
    /// # Errors
    ///
    /// Returns `SignInRequiredError` if no credentials are available, or
    /// `SetupError` if the provider configuration cannot connect.
    pub fn connect(&self) -> Result<(), JsValue> {
        let storage =
            self.connector.connect().map_err(|error| match error {
                crate::domain::ConnectStorageError::NotAuthenticated {
                    ..
                } => authorization_error(AuthorizationError::SignInRequired),
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
        let mut writer = Vec::with_capacity(length as usize);
        storage
            .read_file_at(path, &mut writer, offset..end)
            .await
            .map_err(read_file_error)?;
        Ok(writer)
    }

    /// Returns the latest unsaved credential snapshot JSON, or no value.
    ///
    /// Contains secrets. Use `savePending` to save and acknowledge the update.
    ///
    /// # Errors
    ///
    /// Returns `HostSaveError` if the snapshot cannot be serialized.
    #[wasm_bindgen(js_name = pendingUpdate)]
    pub fn pending_update(&self) -> Result<Option<String>, JsValue> {
        self.connector
            .credential_snapshot(self.saves.committed.get().unwrap_or(0))
            .map(|update| update_json(&update).map_err(host_save_error))
            .transpose()
    }

    #[wasm_bindgen(js_name = saveStatus)]
    pub fn save_status(&self) -> Result<String, JsValue> {
        let generation = self.connector.status().generation;
        let value = if self.saves.committed.get() == Some(generation) {
            json!({"state": "saved", "generation": generation})
        } else if self.saves.failed.get() == Some(generation) {
            json!({"state": "failed", "generation": generation})
        } else if generation > self.saves.initial_generation {
            json!({"state": "pending", "generation": generation})
        } else {
            json!({"state": "clean"})
        };
        encode(value)
    }

    /// Saves pending configuration through a host-owned promise callback.
    ///
    /// The callback receives JSON `{generation, config, fields}`. Copy only
    /// the listed storage-relative paths from `config`; remove absent or null
    /// values. Resolve only after the external write finishes.
    /// Writes are serialized per host and continue if the caller stops
    /// awaiting them. Storage operations neither invoke nor await them.
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
        let connector = self.connector.clone();
        let saves = Rc::clone(&self.saves);
        let (sender, receiver) = futures::channel::oneshot::channel();
        wasm_bindgen_futures::spawn_local(async move {
            let result = saves.save(&connector, &callback).await;
            let _ = sender.send(result);
        });
        receiver
            .await
            .map_err(|_| host_save_error(HostSaveError))?
            .map_err(host_save_error)
    }
}

impl OneDriveBrowserApi {
    fn from_connector(connector: OneDriveConnector) -> Self {
        let initial_generation = connector.status().generation;
        Self {
            connector,
            saves: Rc::new(BrowserSaves {
                initial_generation,
                ..BrowserSaves::default()
            }),
            storage: RefCell::new(None),
            temporary_storage: js_sys::Reflect::get(
                &js_sys::global(),
                &JsValue::from_str("sessionStorage"),
            )
            .unwrap_or(JsValue::UNDEFINED),
        }
    }

    pub fn connector(&self) -> OneDriveConnector {
        self.connector.clone()
    }

    fn storage_key(&self) -> String {
        format!("anymount.onedrive.redirect:{}", self.connector.identity())
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

struct HostSaveError;

#[derive(Default)]
struct BrowserSaves {
    initial_generation: u64,
    committed: Cell<Option<u64>>,
    failed: Cell<Option<u64>>,
    writer: futures::lock::Mutex<()>,
}

impl BrowserSaves {
    async fn save(
        &self,
        connector: &OneDriveConnector,
        callback: &Function,
    ) -> Result<(), HostSaveError> {
        let _writer = self.writer.lock().await;
        while let Some(update) =
            connector.credential_snapshot(self.committed.get().unwrap_or(0))
        {
            if let Err(error) = save_update(callback, &update).await {
                self.failed.set(Some(connector.status().generation));
                return Err(error);
            }
            self.committed.set(Some(update.generation));
            self.failed.set(None);
        }
        Ok(())
    }
}

async fn save_update(
    callback: &Function,
    update: &CredentialUpdate,
) -> Result<(), HostSaveError> {
    let encoded = update_json(update)?;
    let result = callback
        .call1(&JsValue::UNDEFINED, &JsValue::from_str(&encoded))
        .map_err(|_| HostSaveError)?;
    JsFuture::from(Promise::resolve(&result))
        .await
        .map_err(|_| HostSaveError)?;
    Ok(())
}

fn update_json(update: &CredentialUpdate) -> Result<String, HostSaveError> {
    let config =
        serde_json::to_value(&update.config).map_err(|_| HostSaveError)?;
    serde_json::to_string(&json!({
        "generation": update.generation,
        "config": config,
        "fields": update.fields,
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
