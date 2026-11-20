use super::{
    DEFAULT_TOKEN_EXPIRY_BUFFER_SECS, GrantBinding, OneDriveConfig,
    OneDriveStorage, connect_error,
};
use crate::domain::{
    Authorization, AuthorizationError, ConfigurationPersistence,
    ConnectStorageError, CredentialUpdate, DeviceAuthorization, HostSaveError,
    RedirectAuthorization, SaveStatus,
};
use crate::drivers::oauth::{OAuthRuntime, RuntimeFuture};
use futures::{
    FutureExt,
    channel::oneshot,
    future::{Either, Shared, select},
};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, DeviceAuthorizationUrl,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken,
    RequestTokenError, Scope, TokenResponse, TokenUrl,
    basic::{BasicClient, BasicErrorResponseType, BasicTokenResponse},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

const AUTH_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const TOKEN_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const DEVICE_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
// Keep navigation continuations short-lived and bounded independently of grants.
const CONTINUATION_LIFETIME: u64 = 600;
const MAX_CONTINUATION_BYTES: usize = 8192;
// Refuse excess sign-in attempts rather than evicting a still-replayable record.
const MAX_RECENT_ATTEMPTS: usize = 32;

type OAuthClient = oauth2::Client<
    oauth2::basic::BasicErrorResponse,
    BasicTokenResponse,
    oauth2::basic::BasicTokenIntrospectionResponse,
    oauth2::StandardRevocableToken,
    oauth2::basic::BasicRevocationErrorResponse,
    oauth2::EndpointSet,
    oauth2::EndpointSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointSet,
>;

fn client(config: &OneDriveConfig) -> Result<OAuthClient, AuthorizationError> {
    Ok(
        BasicClient::new(ClientId::new(config.expected_binding().client_id))
            .set_auth_uri(
                AuthUrl::new(AUTH_URL.into())
                    .map_err(|_| AuthorizationError::Setup)?,
            )
            .set_token_uri(
                TokenUrl::new(TOKEN_URL.into())
                    .map_err(|_| AuthorizationError::Setup)?,
            )
            .set_device_authorization_url(
                DeviceAuthorizationUrl::new(DEVICE_URL.into())
                    .map_err(|_| AuthorizationError::Setup)?,
            ),
    )
}

#[derive(Clone, Serialize, Deserialize)]
struct Continuation {
    identity: String,
    binding: GrantBinding,
    generation: u64,
    redirect: String,
    state: String,
    verifier: String,
    issued_at: u64,
    expires_at: u64,
}

enum Attempt {
    Redirect(Continuation),
    Device {
        details: oauth2::StandardDeviceAuthorizationResponse,
        expires_at: u64,
        cancel: Option<oneshot::Sender<()>>,
        polling: bool,
    },
}

/// Reports safe lifecycle diagnostics without credential-derived identifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OneDriveStatus {
    pub stage: &'static str,
    pub generation: u64,
    pub outcome: Option<AuthorizationError>,
    pub save: SaveStatus,
}
#[derive(Clone)]
pub struct RequestCredential {
    pub token: Arc<str>,
    pub generation: u64,
}
impl fmt::Debug for RequestCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestCredential")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}
type RefreshFlight =
    Shared<RuntimeFuture<Result<RequestCredential, AuthorizationError>>>;

struct State {
    config: OneDriveConfig,
    request_token: Option<Arc<str>>,
    generation: u64,
    saved_generation: u64,
    save_status: SaveStatus,
    refresh: Option<(u64, RefreshFlight)>,
    epoch: u64,
    attempt: Option<Attempt>,
    consumed: HashMap<String, u64>,
    waiters: Vec<(u64, oneshot::Sender<u64>)>,
}
struct Session {
    identity: String,
    runtime: OAuthRuntime,
    state: Mutex<State>,
    save_lock: futures::lock::Mutex<()>,
}

/// Shares a live grant, refresh result, and latest unsaved provider configuration.
///
/// Clone shares ownership; constructing another session intentionally does not.
/// Hosts must prevent concurrent processes or tabs owning the same saved record.
#[derive(Clone)]
pub struct OneDriveSession(Arc<Session>);
impl fmt::Debug for OneDriveSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.0.state.lock().expect("session state");
        f.debug_struct("OneDriveSession")
            .field("generation", &state.generation)
            .field("save_status", &state.save_status)
            .finish_non_exhaustive()
    }
}
impl OneDriveSession {
    /// Creates a host-owned identity using real portable asynchronous I/O.
    ///
    /// # Errors
    /// Rejects unsupported resource endpoints and mismatched grant bindings.
    pub fn new(
        config: OneDriveConfig,
        identity: String,
    ) -> Result<Self, AuthorizationError> {
        Self::with_runtime(config, identity, OAuthRuntime::new()?)
    }
    /// Creates a session with an injected HTTP, execution, and clock port.
    ///
    /// # Errors
    /// Rejects unsupported resource endpoints and mismatched grant bindings.
    pub fn with_runtime(
        config: OneDriveConfig,
        identity: String,
        runtime: OAuthRuntime,
    ) -> Result<Self, AuthorizationError> {
        config.validate()?;
        let generation = config.credential_generation;
        let request_token = config.access_token.as_deref().map(Arc::from);
        Ok(Self(Arc::new(Session {
            identity,
            runtime,
            state: Mutex::new(State {
                config,
                request_token,
                generation,
                saved_generation: generation,
                save_status: SaveStatus::Clean,
                refresh: None,
                epoch: 0,
                attempt: None,
                consumed: HashMap::new(),
                waiters: Vec::new(),
            }),
            save_lock: futures::lock::Mutex::new(()),
        })))
    }
    /// Creates a resource connection sharing this session, without network or UI.
    ///
    /// # Errors
    /// Returns sign-in required when no access or refresh credentials exist.
    pub fn connect(&self) -> Result<OneDriveStorage, ConnectStorageError> {
        let state = self.0.state.lock().expect("session state");
        if state.config.access_token.is_none()
            && state.config.refresh_token.is_none()
        {
            return Err(connect_error(AuthorizationError::SignInRequired));
        }
        Ok(OneDriveStorage::new(&state.config, self.clone()))
    }

    /// Reports lifecycle stage, outcome, generation, and independent save state.
    pub fn status(&self) -> OneDriveStatus {
        let state = self.0.state.lock().expect("session state");
        let (stage, outcome) = match &state.attempt {
            Some(Attempt::Redirect(_)) => ("redirect.awaiting_callback", None),
            Some(Attempt::Device { polling: true, .. }) => {
                ("device.polling", None)
            }
            Some(Attempt::Device { .. }) => ("device.awaiting_user", None),
            None => match &state.refresh {
                Some((_, flight)) => match flight.peek() {
                    Some(Err(error)) => ("refresh.failed", Some(*error)),
                    _ => ("refresh.pending", None),
                },
                None => ("credentials.ready", None),
            },
        };
        OneDriveStatus {
            stage,
            generation: state.generation,
            outcome,
            save: state.save_status,
        }
    }
    pub fn runtime(&self) -> &OAuthRuntime {
        &self.0.runtime
    }

    /// Returns the host-owned configuration identity, not a token identifier.
    pub fn identity(&self) -> &str {
        &self.0.identity
    }

    /// Retains a provider-confirmed terminal rejection for this generation.
    pub fn reject_credentials(&self, generation: u64) {
        let mut state = self.0.state.lock().expect("session state");
        if state.generation == generation {
            state.request_token = None;
            let future: RuntimeFuture<_> =
                Box::pin(async { Err(AuthorizationError::SignInRequired) });
            state.refresh = Some((generation, future.shared()));
        }
    }

    /// Waits for a newer credential generation without consuming its snapshot.
    pub async fn wait_for_update(&self, after_generation: u64) -> u64 {
        let receiver = {
            let mut state = self.0.state.lock().expect("session state");
            if state.generation > after_generation {
                return state.generation;
            }
            let (sender, receiver) = oneshot::channel();
            state.waiters.retain(|(_, sender)| !sender.is_canceled());
            state.waiters.push((after_generation, sender));
            receiver
        };
        receiver.await.unwrap_or(after_generation)
    }

    /// Gets provider request credentials, coordinating one detached refresh.
    ///
    /// Completed rotations survive cancellation of every requesting waiter.
    /// A failed exchange remains shared; ambiguous refreshes are never replayed.
    pub async fn request_credentials(
        &self,
        rejected_generation: Option<u64>,
    ) -> Result<RequestCredential, AuthorizationError> {
        let flight = {
            let mut state = self.0.state.lock().expect("session state");
            let rejected = rejected_generation == Some(state.generation);
            if rejected {
                state.request_token = None;
            }
            let buffer = if state.config.refresh_token.is_some() {
                state
                    .config
                    .token_expiry_buffer_secs
                    .unwrap_or(DEFAULT_TOKEN_EXPIRY_BUFFER_SECS)
            } else {
                0
            };
            let expired = state.config.expires_at.is_some_and(|expiry| {
                expiry <= self.0.runtime.now().saturating_add(buffer)
            });
            if !rejected && !expired {
                if let Some(token) = &state.request_token {
                    return Ok(RequestCredential {
                        token: Arc::clone(token),
                        generation: state.generation,
                    });
                }
            }
            let existing = state
                .refresh
                .as_ref()
                .filter(|(generation, _)| *generation == state.generation)
                .map(|(_, flight)| flight.clone());
            if let Some(flight) = existing {
                flight
            } else {
                let refresh = state
                    .config
                    .refresh_token
                    .clone()
                    .ok_or(AuthorizationError::SignInRequired)?;
                let generation = state.generation;
                let oauth = client(&state.config)?;
                let session = self.clone();
                let (sender, receiver) = oneshot::channel();
                let future: RuntimeFuture<_> = Box::pin(async move {
                    receiver.await.unwrap_or(Err(AuthorizationError::Temporary))
                });
                let flight = future.shared();
                state.refresh = Some((generation, flight.clone()));
                // Detached ownership starts before any waiter can be cancelled.
                self.0.runtime.spawn(Box::pin(async move {
                    let result = async {
                        let response = oauth
                            .exchange_refresh_token(&RefreshToken::new(refresh))
                            .request_async(&session.0.runtime)
                            .await
                            .map_err(classify_basic)?;
                        let mut state =
                            session.0.state.lock().expect("session state");
                        if state.generation == generation {
                            install(
                                &mut state,
                                response,
                                true,
                                session.0.runtime.now(),
                            );
                        }
                        state
                            .request_token
                            .as_ref()
                            .map(|token| RequestCredential {
                                token: Arc::clone(token),
                                generation: state.generation,
                            })
                            .ok_or(AuthorizationError::SignInRequired)
                    }
                    .await;
                    let _ = sender.send(result);
                }));
                flight
            }
        };
        flight.await
    }
    /// Starts host-presented PKCE navigation.
    pub async fn begin_redirect(
        &self,
        redirect_uri: &str,
    ) -> Result<RedirectAuthorization, AuthorizationError> {
        let redirect = url::Url::parse(redirect_uri)
            .map_err(|_| AuthorizationError::Setup)?;
        if redirect.query().is_some()
            || redirect.fragment().is_some()
            || !redirect.username().is_empty()
            || redirect.password().is_some()
        {
            return Err(AuthorizationError::Setup);
        }
        if redirect.scheme() != "https"
            && redirect.scheme() != "anymount"
            && !(redirect.scheme() == "http"
                && matches!(
                    redirect.host_str(),
                    Some("localhost" | "127.0.0.1" | "[::1]")
                ))
        {
            return Err(AuthorizationError::Setup);
        }
        let mut state = self.0.state.lock().expect("session state");
        cancel_attempt(&mut state);
        state
            .consumed
            .retain(|_, expiry| *expiry > self.0.runtime.now());
        if state.consumed.len() >= MAX_RECENT_ATTEMPTS {
            return Err(AuthorizationError::Setup);
        }
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let oauth = client(&state.config)?.set_redirect_uri(
            RedirectUrl::new(redirect.to_string())
                .map_err(|_| AuthorizationError::Setup)?,
        );
        let (url, correlation) = oauth
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(challenge)
            .add_scope(Scope::new("Files.ReadWrite".into()))
            .add_scope(Scope::new("offline_access".into()))
            .url();
        let now = self.0.runtime.now();
        let record = Continuation {
            identity: self.0.identity.clone(),
            binding: state.config.expected_binding(),
            generation: state.generation,
            redirect: redirect.to_string(),
            state: correlation.secret().clone(),
            verifier: verifier.secret().clone(),
            issued_at: now,
            expires_at: now.saturating_add(CONTINUATION_LIFETIME),
        };
        let continuation = serde_json::to_string(&record)
            .map_err(|_| AuthorizationError::Setup)?;
        if continuation.len() > MAX_CONTINUATION_BYTES {
            return Err(AuthorizationError::Setup);
        }
        let expires_at = record.expires_at;
        state.attempt = Some(Attempt::Redirect(record));
        Ok(RedirectAuthorization {
            authorization_url: url.into(),
            continuation,
            expires_at,
        })
    }
    /// Restores a host-retained continuation after full-page navigation.
    ///
    /// The host must retain only its newest attempt and atomically remove it
    /// before completion. Long-lived configurations never store continuations.
    pub fn resume_redirect(
        &self,
        continuation: &str,
    ) -> Result<(), AuthorizationError> {
        if continuation.len() > MAX_CONTINUATION_BYTES {
            return Err(AuthorizationError::InvalidCallback);
        }
        let record: Continuation = serde_json::from_str(continuation)
            .map_err(|_| AuthorizationError::InvalidCallback)?;
        let mut state = self.0.state.lock().expect("session state");
        state
            .consumed
            .retain(|_, expiry| *expiry > self.0.runtime.now());
        if state.consumed.len() >= MAX_RECENT_ATTEMPTS {
            return Err(AuthorizationError::Setup);
        }
        if record.identity != self.0.identity
            || record.binding != state.config.expected_binding()
            || record.generation != state.generation
            || record.state.is_empty()
            || record.verifier.len() < 43
            || record.verifier.len() > 128
            || record.expires_at.saturating_sub(record.issued_at)
                > CONTINUATION_LIFETIME
            || record.issued_at > self.0.runtime.now()
            || state.consumed.contains_key(&record.state)
            || state.attempt.is_some()
        {
            return Err(AuthorizationError::InvalidCallback);
        }
        if record.expires_at <= self.0.runtime.now() {
            return Err(AuthorizationError::Expired);
        }
        state.epoch += 1;
        state.attempt = Some(Attempt::Redirect(record));
        Ok(())
    }
    /// Checks a callback before the host consumes its persisted continuation.
    ///
    /// # Errors
    ///
    /// Rejects stale, malformed, expired, or mismatched callbacks without
    /// changing the active attempt or its persisted host record.
    pub fn validate_redirect_callback(
        &self,
        callback: &str,
    ) -> Result<(), AuthorizationError> {
        let state = self.0.state.lock().expect("session state");
        let Some(Attempt::Redirect(record)) = &state.attempt else {
            return Err(AuthorizationError::InvalidCallback);
        };
        callback_fields(record, callback, self.0.runtime.now()).map(|_| ())
    }

    /// Consumes a matching callback once and replaces the entire grant.
    pub async fn complete_redirect(
        &self,
        callback: &str,
    ) -> Result<(), AuthorizationError> {
        let (record, epoch, config, code) = {
            let mut state = self.0.state.lock().expect("session state");
            let Some(Attempt::Redirect(record)) = state.attempt.as_ref() else {
                return Err(AuthorizationError::InvalidCallback);
            };
            if record.expires_at <= self.0.runtime.now() {
                cancel_attempt(&mut state);
                return Err(AuthorizationError::Expired);
            }
            let (code, error) =
                callback_fields(record, callback, self.0.runtime.now())?;
            let record = record.clone();
            state
                .consumed
                .insert(record.state.clone(), record.expires_at);
            state.attempt = None;
            if let Some(error) = error {
                return Err(match error.as_str() {
                    "access_denied" => AuthorizationError::Declined,
                    "temporarily_unavailable" | "server_error" => {
                        AuthorizationError::Temporary
                    }
                    _ => AuthorizationError::Setup,
                });
            }
            (
                record,
                state.epoch,
                state.config.clone(),
                code.ok_or(AuthorizationError::InvalidCallback)?,
            )
        };
        let oauth = client(&config)?.set_redirect_uri(
            RedirectUrl::new(record.redirect)
                .map_err(|_| AuthorizationError::Setup)?,
        );
        let response = oauth
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(PkceCodeVerifier::new(record.verifier))
            .request_async(&self.0.runtime)
            .await
            .map_err(classify_basic)?;
        let mut state = self.0.state.lock().expect("session state");
        if state.epoch != epoch {
            return Err(AuthorizationError::Cancelled);
        }
        install(&mut state, response, false, self.0.runtime.now());
        Ok(())
    }
    /// Obtains provider device instructions without synthesizing complete URLs.
    pub async fn begin_device(
        &self,
    ) -> Result<DeviceAuthorization, AuthorizationError> {
        let (config, epoch) = {
            let mut state = self.0.state.lock().expect("session state");
            cancel_attempt(&mut state);
            (state.config.clone(), state.epoch)
        };
        let details: oauth2::StandardDeviceAuthorizationResponse =
            client(&config)?
                .exchange_device_code()
                .add_scope(Scope::new("Files.ReadWrite".into()))
                .add_scope(Scope::new("offline_access".into()))
                .request_async(&self.0.runtime)
                .await
                .map_err(classify_basic)?;
        let expires_at = self
            .0
            .runtime
            .now()
            .saturating_add(details.expires_in().as_secs());
        let instructions = DeviceAuthorization {
            verification_uri: details.verification_uri().to_string(),
            user_code: details.user_code().secret().clone(),
            verification_uri_complete: details
                .verification_uri_complete()
                .map(|url| url.secret().clone()),
            expires_at,
        };
        let mut state = self.0.state.lock().expect("session state");
        if state.epoch != epoch {
            return Err(AuthorizationError::Cancelled);
        }
        state.attempt = Some(Attempt::Device {
            details,
            expires_at,
            cancel: None,
            polling: false,
        });
        Ok(instructions)
    }

    /// Polls using oauth2's interval, slowdown, and expiration machinery.
    pub async fn poll_device(&self) -> Result<(), AuthorizationError> {
        let (details, expires_at, receiver, epoch, config) = {
            let mut state = self.0.state.lock().expect("session state");
            let Some(Attempt::Device {
                details,
                expires_at,
                cancel,
                polling,
            }) = state.attempt.as_mut()
            else {
                return Err(AuthorizationError::InvalidCallback);
            };
            if *polling {
                return Err(AuthorizationError::InvalidCallback);
            }
            if *expires_at <= self.0.runtime.now() {
                return Err(AuthorizationError::Expired);
            }
            *polling = true;
            let (sender, receiver) = oneshot::channel();
            *cancel = Some(sender);
            (
                details.clone(),
                *expires_at,
                receiver,
                state.epoch,
                state.config.clone(),
            )
        };
        let oauth = client(&config)?;
        let poll = async {
            // RFC 8628 requires waiting the provider interval before polling.
            self.0.runtime.sleep(details.interval()).await;
            let now = self.0.runtime.now();
            if now >= expires_at {
                return Err(AuthorizationError::Expired);
            }
            let remaining = Duration::from_secs(expires_at - now);
            let clock = Arc::new(AtomicU64::new(now));
            let time = Arc::clone(&clock);
            let runtime = self.0.runtime.clone();
            oauth
                .exchange_device_access_token(&details)
                .set_time_fn(move || {
                    chrono::DateTime::from_timestamp(
                        time.load(Ordering::Relaxed) as i64,
                        0,
                    )
                    .expect("valid clock")
                })
                .request_async(
                    &self.0.runtime,
                    move |duration| {
                        let runtime = runtime.clone();
                        let clock = Arc::clone(&clock);
                        async move {
                            runtime.sleep(duration).await;
                            clock.store(runtime.now(), Ordering::Relaxed);
                        }
                    },
                    Some(remaining),
                )
                .await
                .map_err(classify_device)
        };
        let result = match select(Box::pin(poll), receiver).await {
            Either::Left((result, _)) => result,
            Either::Right(_) => return Err(AuthorizationError::Cancelled),
        };
        let mut state = self.0.state.lock().expect("session state");
        if state.epoch != epoch {
            return Err(AuthorizationError::Cancelled);
        }
        state.attempt = None;
        install(&mut state, result?, false, self.0.runtime.now());
        Ok(())
    }
    /// Cancels local interaction without revoking or clearing the existing grant.
    pub fn cancel(&self) {
        cancel_attempt(&mut self.0.state.lock().expect("session state"));
    }
    /// Clears only a completed temporary renewal failure for explicit recovery.
    pub fn retry_renewal(&self) -> bool {
        let mut state = self.0.state.lock().expect("session state");
        if matches!(
            state.refresh.as_ref().and_then(|(_, flight)| flight.peek()),
            Some(Err(AuthorizationError::Temporary))
        ) {
            state.refresh = None;
            true
        } else {
            false
        }
    }
    /// Retains the newest unsaved snapshot even after failed or cancelled saves.
    pub fn pending_update(&self) -> Option<CredentialUpdate> {
        let state = self.0.state.lock().expect("session state");
        (state.generation > state.saved_generation).then(|| CredentialUpdate {
            generation: state.generation,
            config: Box::new(state.config.clone()),
        })
    }
    pub fn save_status(&self) -> SaveStatus {
        self.0.state.lock().expect("session state").save_status
    }
    /// Serializes host saves, never holding a credential lock while saving.
    ///
    /// # Errors
    /// Retains the latest snapshot after host failure; retry does not refresh.
    pub async fn save_pending(
        &self,
        store: &dyn ConfigurationPersistence,
    ) -> Result<(), HostSaveError> {
        let _guard = self.0.save_lock.lock().await;
        while let Some(update) = self.pending_update() {
            let result = store.save(&update).await;
            let mut state = self.0.state.lock().expect("session state");
            match result {
                Ok(()) => {
                    state.saved_generation = update.generation;
                    state.save_status = if state.generation == update.generation
                    {
                        SaveStatus::Saved {
                            generation: update.generation,
                        }
                    } else {
                        SaveStatus::Pending {
                            generation: state.generation,
                        }
                    };
                }
                Err(error) => {
                    state.save_status = SaveStatus::Failed {
                        generation: state.generation,
                    };
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

fn callback_fields(
    record: &Continuation,
    callback: &str,
    now: u64,
) -> Result<(Option<String>, Option<String>), AuthorizationError> {
    if record.expires_at <= now {
        return Err(AuthorizationError::Expired);
    }
    if callback.len() > 8192 {
        return Err(AuthorizationError::InvalidCallback);
    }
    let callback = url::Url::parse(callback)
        .map_err(|_| AuthorizationError::InvalidCallback)?;
    let mut base = callback.clone();
    base.set_query(None);
    base.set_fragment(None);
    if base.as_str() != record.redirect || callback.fragment().is_some() {
        return Err(AuthorizationError::InvalidCallback);
    }
    let (mut state, mut code, mut error) = (None, None, None);
    for (name, value) in callback.query_pairs() {
        let slot = match name.as_ref() {
            "state" => &mut state,
            "code" => &mut code,
            "error" => &mut error,
            _ => continue,
        };
        if slot.replace(value.into_owned()).is_some() {
            return Err(AuthorizationError::InvalidCallback);
        }
    }
    if state.as_deref() != Some(record.state.as_str())
        || code.is_some() == error.is_some()
        || code.as_ref().is_some_and(String::is_empty)
    {
        return Err(AuthorizationError::InvalidCallback);
    }
    Ok((code, error))
}

fn cancel_attempt(state: &mut State) {
    state.epoch += 1;
    match state.attempt.take() {
        Some(Attempt::Redirect(record)) => {
            state.consumed.insert(record.state, record.expires_at);
        }
        Some(Attempt::Device {
            cancel: Some(sender),
            ..
        }) => {
            let _ = sender.send(());
        }
        _ => {}
    }
}
fn install(
    state: &mut State,
    response: BasicTokenResponse,
    renewal: bool,
    now: u64,
) {
    state.config.access_token = Some(response.access_token().secret().clone());
    state.request_token =
        Some(Arc::from(response.access_token().secret().as_str()));
    let replacement =
        response.refresh_token().map(|token| token.secret().clone());
    if !renewal || replacement.is_some() {
        state.config.refresh_token = replacement;
    }
    state.config.expires_at = response
        .expires_in()
        .map(|duration| now.saturating_add(duration.as_secs()));
    if !renewal || response.scopes().is_some() {
        state.config.scopes = response.scopes().map(|scopes| {
            scopes.iter().map(|scope| scope.to_string()).collect()
        });
    }
    state.config.binding = Some(state.config.expected_binding());
    state.generation += 1;
    state.config.credential_generation = state.generation;
    state.refresh = None;
    state.save_status = SaveStatus::Pending {
        generation: state.generation,
    };
    for (_, sender) in state.waiters.drain(..) {
        let _ = sender.send(state.generation);
    }
}
fn classify_basic(
    error: RequestTokenError<
        AuthorizationError,
        oauth2::basic::BasicErrorResponse,
    >,
) -> AuthorizationError {
    match error {
        RequestTokenError::Request(error) => error,
        RequestTokenError::ServerResponse(response) => {
            classify_code(response.error())
        }
        _ => AuthorizationError::Temporary,
    }
}
fn classify_code(code: &BasicErrorResponseType) -> AuthorizationError {
    match code {
        BasicErrorResponseType::InvalidGrant => {
            AuthorizationError::SignInRequired
        }
        BasicErrorResponseType::Extension(code) => match code.as_str() {
            "interaction_required" | "login_required" | "consent_required" => {
                AuthorizationError::SignInRequired
            }
            "access_denied" | "authorization_declined" => {
                AuthorizationError::Declined
            }
            "temporarily_unavailable" | "server_error" => {
                AuthorizationError::Temporary
            }
            _ => AuthorizationError::Setup,
        },
        _ => AuthorizationError::Setup,
    }
}
fn classify_device(
    error: RequestTokenError<
        AuthorizationError,
        oauth2::DeviceCodeErrorResponse,
    >,
) -> AuthorizationError {
    match error {
        RequestTokenError::Request(error) => error,
        RequestTokenError::ServerResponse(response) => match response.error() {
            oauth2::DeviceCodeErrorResponseType::AccessDenied => {
                AuthorizationError::Declined
            }
            oauth2::DeviceCodeErrorResponseType::ExpiredToken => {
                AuthorizationError::Expired
            }
            oauth2::DeviceCodeErrorResponseType::Basic(code) => {
                classify_code(code)
            }
            _ => AuthorizationError::Temporary,
        },
        _ => AuthorizationError::Temporary,
    }
}

#[async_trait::async_trait(?Send)]
impl Authorization for OneDriveSession {
    async fn begin_redirect(
        &self,
        redirect: &str,
    ) -> Result<RedirectAuthorization, AuthorizationError> {
        self.begin_redirect(redirect).await
    }
    fn resume_redirect(&self, record: &str) -> Result<(), AuthorizationError> {
        self.resume_redirect(record)
    }
    async fn complete_redirect(
        &self,
        callback: &str,
    ) -> Result<(), AuthorizationError> {
        self.complete_redirect(callback).await
    }
    async fn begin_device(
        &self,
    ) -> Result<DeviceAuthorization, AuthorizationError> {
        self.begin_device().await
    }
    async fn poll_device(&self) -> Result<(), AuthorizationError> {
        self.poll_device().await
    }
    fn cancel(&self) {
        self.cancel();
    }
    fn retry_renewal(&self) -> bool {
        self.retry_renewal()
    }
    fn pending_update(&self) -> Option<CredentialUpdate> {
        self.pending_update()
    }
    fn save_status(&self) -> SaveStatus {
        self.save_status()
    }
    async fn save_pending(
        &self,
        store: &dyn ConfigurationPersistence,
    ) -> Result<(), HostSaveError> {
        self.save_pending(store).await
    }
}
