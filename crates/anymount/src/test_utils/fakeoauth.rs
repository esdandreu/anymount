//! Deterministic protocol-boundary fakes for OAuth and OneDrive consumers.

use crate::domain::{
    AuthorizationError, ConfigurationPersistence, CredentialUpdate,
    HostSaveError,
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

#[derive(Default)]
pub struct FakeConfigurationPersistence {
    pub received: Mutex<Vec<u64>>,
    pub saved: Mutex<Vec<serde_json::Value>>,
    pub fail: Mutex<bool>,
    pub gate: Mutex<Option<oneshot::Receiver<()>>>,
}
#[async_trait::async_trait(?Send)]
impl ConfigurationPersistence for FakeConfigurationPersistence {
    async fn save(
        &self,
        update: &CredentialUpdate,
    ) -> Result<(), HostSaveError> {
        self.received
            .lock()
            .expect("received")
            .push(update.generation);
        let gate = self.gate.lock().expect("gate").take();
        if let Some(gate) = gate {
            let _ = gate.await;
        }
        if *self.fail.lock().expect("fail") {
            return Err(HostSaveError);
        }
        self.saved.lock().expect("saved").push(
            serde_json::to_value(&update.config).expect("config fixture"),
        );
        Ok(())
    }
}
