//! Deterministic protocol-boundary fakes for OAuth and OneDrive consumers.

use crate::domain::{
    AuthorizationError, ConfigRecordId, ConfigRepository, CredentialUpdate,
    LoadedConfig, SaveCredentialsError,
};
use crate::drivers::oauth::{OAuthRuntime, OAuthRuntimePort, RuntimeFuture};
use futures::channel::oneshot;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone)]
pub struct RecordedRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
impl std::fmt::Debug for RecordedRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecordedRequest([redacted])")
    }
}
enum Response {
    Ready(Result<oauth2::HttpResponse, AuthorizationError>),
    Deferred(oneshot::Receiver<oauth2::HttpResponse>),
}
struct State {
    now: u64,
    responses: VecDeque<Response>,
    requests: Vec<RecordedRequest>,
    sleeps: Vec<Duration>,
}
#[derive(Clone)]
pub struct FakeOAuthRuntime(Arc<Mutex<State>>);
impl FakeOAuthRuntime {
    pub fn new(now: u64) -> Self {
        Self(Arc::new(Mutex::new(State {
            now,
            responses: VecDeque::new(),
            requests: Vec::new(),
            sleeps: Vec::new(),
        })))
    }
    pub fn runtime(&self) -> OAuthRuntime {
        OAuthRuntime::with_port(self.clone())
    }
    pub fn push_json(&self, status: u16, body: serde_json::Value) {
        self.push_body(
            status,
            serde_json::to_vec(&body).expect("JSON fixture"),
        );
    }
    pub fn push_body(&self, status: u16, body: Vec<u8>) {
        let response = oauth2::http::Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(body)
            .expect("HTTP fixture");
        self.0
            .lock()
            .expect("fake state")
            .responses
            .push_back(Response::Ready(Ok(response)));
    }
    pub fn push_error(&self, error: AuthorizationError) {
        self.0
            .lock()
            .expect("fake state")
            .responses
            .push_back(Response::Ready(Err(error)));
    }
    pub fn defer_response(&self) -> oneshot::Sender<oauth2::HttpResponse> {
        let (sender, receiver) = oneshot::channel();
        self.0
            .lock()
            .expect("fake state")
            .responses
            .push_back(Response::Deferred(receiver));
        sender
    }
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.0.lock().expect("fake state").requests.clone()
    }
    pub fn sleeps(&self) -> Vec<Duration> {
        self.0.lock().expect("fake state").sleeps.clone()
    }
    pub fn advance(&self, seconds: u64) {
        self.0.lock().expect("fake state").now += seconds;
    }
}
impl OAuthRuntimePort for FakeOAuthRuntime {
    fn request(
        &self,
        request: oauth2::HttpRequest,
    ) -> RuntimeFuture<Result<oauth2::HttpResponse, AuthorizationError>> {
        let response = {
            let mut state = self.0.lock().expect("fake state");
            state.requests.push(RecordedRequest {
                method: request.method().to_string(),
                url: request.uri().to_string(),
                headers: request
                    .headers()
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.to_string(),
                            value.to_str().expect("fixture header").into(),
                        )
                    })
                    .collect(),
                body: request.into_body(),
            });
            state
                .responses
                .pop_front()
                .expect("unexpected protocol request")
        };
        Box::pin(async move {
            match response {
                Response::Ready(result) => result,
                Response::Deferred(receiver) => {
                    receiver.await.map_err(|_| AuthorizationError::Temporary)
                }
            }
        })
    }
    fn sleep(&self, duration: Duration) -> RuntimeFuture<()> {
        let this = self.clone();
        Box::pin(async move {
            {
                let mut state = this.0.lock().expect("fake state");
                state.now += duration.as_secs();
                state.sleeps.push(duration);
            }
            #[cfg(not(target_arch = "wasm32"))]
            tokio::task::yield_now().await;
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(0).await;
        })
    }
    fn spawn(&self, future: RuntimeFuture<()>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            tokio::spawn(future);
        }
        #[cfg(target_arch = "wasm32")]
        {
            wasm_bindgen_futures::spawn_local(future);
        }
    }
    fn now(&self) -> u64 {
        self.0.lock().expect("fake state").now
    }
}

pub struct ObservedConfigRepository<R> {
    repository: R,
    outcomes: std::sync::mpsc::Sender<(u64, Result<(), SaveCredentialsError>)>,
    pause: Mutex<Option<(std::sync::mpsc::Sender<u64>, oneshot::Receiver<()>)>>,
}

impl<R> ObservedConfigRepository<R> {
    pub fn new(
        repository: R,
    ) -> (
        Self,
        std::sync::mpsc::Receiver<(u64, Result<(), SaveCredentialsError>)>,
    ) {
        let (outcomes, receiver) = std::sync::mpsc::channel();
        (
            Self {
                repository,
                outcomes,
                pause: Mutex::new(None),
            },
            receiver,
        )
    }

    pub fn pause_next_save(
        &self,
    ) -> (std::sync::mpsc::Receiver<u64>, oneshot::Sender<()>) {
        let (started, started_rx) = std::sync::mpsc::channel();
        let (release, release_rx) = oneshot::channel();
        *self.pause.lock().expect("pause") = Some((started, release_rx));
        (started_rx, release)
    }
}

#[async_trait::async_trait(?Send)]
impl<R: ConfigRepository> ConfigRepository for ObservedConfigRepository<R> {
    fn list(&self) -> Box<dyn Iterator<Item = LoadedConfig> + '_> {
        self.repository.list()
    }

    async fn save_credentials(
        &self,
        record_id: &ConfigRecordId,
        update: CredentialUpdate,
    ) -> Result<(), SaveCredentialsError> {
        let generation = update.generation;
        let pause = self.pause.lock().expect("pause").take();
        if let Some((started, release)) = pause {
            let _ = started.send(generation);
            let _ = release.await;
        }
        let outcome = self.repository.save_credentials(record_id, update).await;
        let _ = self.outcomes.send((generation, outcome));
        outcome
    }
}
