//! Async HTTP, timers, and detached work for portable OAuth providers.

use crate::domain::AuthorizationError;
use oauth2::{HttpRequest, HttpResponse};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

// Bound slow upstreams while allowing time for large storage responses.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// A runtime future that remains Send on native platforms.
#[cfg(not(target_arch = "wasm32"))]
pub type RuntimeFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;
/// A browser runtime future, which may use JavaScript handles.
#[cfg(target_arch = "wasm32")]
pub type RuntimeFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

/// Requires thread-safe runtime state on native platforms.
#[cfg(not(target_arch = "wasm32"))]
pub trait RuntimeBounds: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> RuntimeBounds for T {}

/// Allows browser runtime state to own thread-local handles.
#[cfg(target_arch = "wasm32")]
pub trait RuntimeBounds {}
#[cfg(target_arch = "wasm32")]
impl<T> RuntimeBounds for T {}

/// Supplies protocol-boundary I/O and time; implementations never log requests.
pub trait OAuthRuntimePort: RuntimeBounds {
    fn request(
        &self,
        request: HttpRequest,
    ) -> RuntimeFuture<Result<HttpResponse, AuthorizationError>>;
    fn sleep(&self, duration: Duration) -> RuntimeFuture<()>;
    fn spawn(&self, future: RuntimeFuture<()>);
    fn now(&self) -> u64;
}

/// Shares asynchronous protocol I/O with platform-specific execution adapters.
#[derive(Clone)]
pub struct OAuthRuntime(Arc<dyn OAuthRuntimePort>);

impl std::fmt::Debug for OAuthRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OAuthRuntime")
    }
}

impl OAuthRuntime {
    /// Creates real asynchronous HTTP and timers for the current platform.
    ///
    /// # Errors
    ///
    /// Returns a setup error if the native TLS client cannot initialize.
    pub fn new() -> Result<Self, AuthorizationError> {
        Ok(Self::with_port(PlatformRuntime::new()?))
    }
    /// Injects a host runtime or deterministic protocol-boundary fake.
    pub fn with_port(port: impl OAuthRuntimePort + 'static) -> Self {
        Self(Arc::new(port))
    }
    pub fn request(
        &self,
        request: HttpRequest,
    ) -> RuntimeFuture<Result<HttpResponse, AuthorizationError>> {
        self.0.request(request)
    }
    pub fn sleep(&self, duration: Duration) -> RuntimeFuture<()> {
        self.0.sleep(duration)
    }
    pub fn spawn(&self, future: RuntimeFuture<()>) {
        self.0.spawn(future);
    }
    pub fn now(&self) -> u64 {
        self.0.now()
    }
}

impl<'a> oauth2::AsyncHttpClient<'a> for OAuthRuntime {
    type Error = AuthorizationError;
    type Future = RuntimeFuture<Result<HttpResponse, AuthorizationError>>;
    fn call(&'a self, request: HttpRequest) -> Self::Future {
        self.request(request)
    }
}

struct PlatformRuntime {
    #[cfg(not(target_arch = "wasm32"))]
    client: reqwest::Client,
}

impl PlatformRuntime {
    fn new() -> Result<Self, AuthorizationError> {
        Ok(Self {
            #[cfg(not(target_arch = "wasm32"))]
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| AuthorizationError::Setup)?,
        })
    }
}

impl OAuthRuntimePort for PlatformRuntime {
    fn request(
        &self,
        request: HttpRequest,
    ) -> RuntimeFuture<Result<HttpResponse, AuthorizationError>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let client = self.client.clone();
            let limit = response_limit(&request);
            Box::pin(request_with_timeout(
                async move {
                    let (parts, body) = request.into_parts();
                    let mut response = client
                        .request(parts.method, parts.uri.to_string())
                        .headers(parts.headers)
                        .body(body)
                        .send()
                        .await
                        .map_err(|_| AuthorizationError::Temporary)?;
                    let mut builder = oauth2::http::Response::builder()
                        .status(response.status());
                    for (name, value) in response.headers() {
                        builder = builder.header(name, value);
                    }
                    let mut body = Vec::new();
                    while let Some(chunk) = response
                        .chunk()
                        .await
                        .map_err(|_| AuthorizationError::Temporary)?
                    {
                        if body.len().saturating_add(chunk.len()) > limit {
                            return Err(AuthorizationError::Temporary);
                        }
                        body.extend_from_slice(&chunk);
                    }
                    builder
                        .body(body)
                        .map_err(|_| AuthorizationError::Temporary)
                },
                REQUEST_TIMEOUT,
            ))
        }
        #[cfg(target_arch = "wasm32")]
        {
            Box::pin(request_with_timeout(
                browser_request(request),
                REQUEST_TIMEOUT,
            ))
        }
    }
    fn sleep(&self, duration: Duration) -> RuntimeFuture<()> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Box::pin(tokio::time::sleep(duration))
        }
        #[cfg(target_arch = "wasm32")]
        {
            Box::pin(gloo_timers::future::TimeoutFuture::new(
                duration.as_millis().min(u128::from(u32::MAX)) as u32,
            ))
        }
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
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        }
        #[cfg(target_arch = "wasm32")]
        {
            (js_sys::Date::now() / 1000.0) as u64
        }
    }
}

async fn request_with_timeout(
    request: impl Future<Output = Result<HttpResponse, AuthorizationError>>,
    duration: Duration,
) -> Result<HttpResponse, AuthorizationError> {
    #[cfg(not(target_arch = "wasm32"))]
    let timer = tokio::time::sleep(duration);
    #[cfg(target_arch = "wasm32")]
    let timer = gloo_timers::future::TimeoutFuture::new(
        duration.as_millis().min(u128::from(u32::MAX)) as u32,
    );
    futures::pin_mut!(request, timer);
    match futures::future::select(request, timer).await {
        futures::future::Either::Left((result, _)) => result,
        futures::future::Either::Right(((), _)) => {
            Err(AuthorizationError::Temporary)
        }
    }
}

// Metadata can be larger than a range; do not buffer a full ignored download.
fn response_limit(request: &HttpRequest) -> usize {
    request
        .headers()
        .get(oauth2::http::header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("bytes="))
        .and_then(|value| value.split_once('-'))
        .and_then(|(start, end)| {
            end.parse::<u64>()
                .ok()?
                .checked_sub(start.parse().ok()?)?
                .checked_add(1)
        })
        .map(|length| {
            length.min(crate::domain::MAX_READ_FILE_AT_RANGE_SIZE as u64)
                as usize
        })
        .unwrap_or(8 * 1024 * 1024)
}

#[cfg(target_arch = "wasm32")]
struct FetchAbort(js_sys::Object);

#[cfg(target_arch = "wasm32")]
impl FetchAbort {
    fn new(options: &web_sys::RequestInit) -> Result<Self, AuthorizationError> {
        use wasm_bindgen::{JsCast, JsValue};
        let constructor = js_sys::Reflect::get(
            &js_sys::global(),
            &JsValue::from_str("AbortController"),
        )
        .map_err(|_| AuthorizationError::Setup)?
        .dyn_into::<js_sys::Function>()
        .map_err(|_| AuthorizationError::Setup)?;
        let controller =
            js_sys::Reflect::construct(&constructor, &js_sys::Array::new())
                .map_err(|_| AuthorizationError::Setup)?;
        let signal =
            js_sys::Reflect::get(&controller, &JsValue::from_str("signal"))
                .map_err(|_| AuthorizationError::Setup)?;
        if !js_sys::Reflect::set(
            options.as_ref(),
            &JsValue::from_str("signal"),
            &signal,
        )
        .map_err(|_| AuthorizationError::Setup)?
        {
            return Err(AuthorizationError::Setup);
        }
        Ok(Self(controller.into()))
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for FetchAbort {
    fn drop(&mut self) {
        use wasm_bindgen::{JsCast, JsValue};
        if let Ok(abort) =
            js_sys::Reflect::get(&self.0, &JsValue::from_str("abort"))
        {
            if let Some(abort) = abort.dyn_ref::<js_sys::Function>() {
                let _ = abort.call0(&self.0);
            }
        }
    }
}
#[cfg(target_arch = "wasm32")]
async fn browser_request(
    request: HttpRequest,
) -> Result<HttpResponse, AuthorizationError> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let limit = response_limit(&request);
    let (parts, body) = request.into_parts();
    let options = web_sys::RequestInit::new();
    let _abort = FetchAbort::new(&options)?;
    options.set_method(parts.method.as_str());
    options.set_mode(web_sys::RequestMode::Cors);
    options.set_credentials(web_sys::RequestCredentials::Omit);
    options.set_redirect(web_sys::RequestRedirect::Error);
    if !body.is_empty() {
        options.set_body(&js_sys::Uint8Array::from(body.as_slice()));
    }
    let request = web_sys::Request::new_with_str_and_init(
        &parts.uri.to_string(),
        &options,
    )
    .map_err(|_| AuthorizationError::Setup)?;
    for (name, value) in &parts.headers {
        request
            .headers()
            .set(
                name.as_str(),
                value.to_str().map_err(|_| AuthorizationError::Setup)?,
            )
            .map_err(|_| AuthorizationError::Setup)?;
    }
    let window = web_sys::window().ok_or(AuthorizationError::Setup)?;
    let response: web_sys::Response =
        JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|_| AuthorizationError::Temporary)?
            .dyn_into()
            .map_err(|_| AuthorizationError::Temporary)?;
    let status = response.status();
    let body = browser_body(response, limit).await?;
    oauth2::http::Response::builder()
        .status(status)
        .body(body)
        .map_err(|_| AuthorizationError::Temporary)
}

#[cfg(target_arch = "wasm32")]
async fn browser_body(
    response: web_sys::Response,
    limit: usize,
) -> Result<Vec<u8>, AuthorizationError> {
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    let Some(stream) = response.body() else {
        return Ok(Vec::new());
    };
    let reader = stream
        .get_reader()
        .dyn_into::<web_sys::ReadableStreamDefaultReader>()
        .map_err(|_| AuthorizationError::Temporary)?;
    let mut bytes = Vec::new();
    loop {
        let result = JsFuture::from(reader.read())
            .await
            .map_err(|_| AuthorizationError::Temporary)?;
        let done = js_sys::Reflect::get(&result, &JsValue::from_str("done"))
            .map_err(|_| AuthorizationError::Temporary)?
            .as_bool()
            .ok_or(AuthorizationError::Temporary)?;
        if done {
            break;
        }
        let chunk = js_sys::Reflect::get(&result, &JsValue::from_str("value"))
            .map_err(|_| AuthorizationError::Temporary)?;
        let chunk = js_sys::Uint8Array::new(&chunk);
        let previous = bytes.len();
        let length = chunk.length() as usize;
        if length > limit.saturating_sub(previous) {
            let _ = JsFuture::from(reader.cancel()).await;
            return Err(AuthorizationError::Temporary);
        }
        bytes.resize(previous + length, 0);
        chunk.copy_to(&mut bytes[previous..]);
    }
    Ok(bytes)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    async fn stalled_response_times_out(send_headers: bool) {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0; 4096];
            stream.read(&mut request).expect("request");
            if send_headers {
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nx",
                    )
                    .expect("response");
            }
            let _ = ready_tx.send(());
            let _ = release_rx.recv();
        });
        let request = oauth2::http::Request::builder()
            .uri(format!("http://{address}/token"))
            .body(Vec::new())
            .expect("request");
        let runtime = OAuthRuntime::new().expect("runtime");
        let mut request = runtime.request(request);
        assert!(matches!(
            futures::future::select(request.as_mut(), ready_rx).await,
            futures::future::Either::Right((Ok(()), _))
        ));
        let result =
            request_with_timeout(request, Duration::from_millis(50)).await;
        assert!(matches!(result, Err(AuthorizationError::Temporary)));
        release_tx.send(()).expect("release server");
        server.join().expect("server");
    }

    #[tokio::test]
    async fn stalled_response_headers_time_out() {
        stalled_response_times_out(false).await;
    }

    #[tokio::test]
    async fn stalled_response_body_times_out() {
        stalled_response_times_out(true).await;
    }

    #[tokio::test]
    async fn ignored_range_cannot_buffer_the_full_download() {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0; 4096];
            stream.read(&mut request).expect("request");
            stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\n01234567890123456789"
            ).expect("response");
        });
        let request = oauth2::http::Request::builder()
            .uri(format!("http://{address}/file"))
            .header("Range", "bytes=0-3")
            .body(Vec::new())
            .expect("request");
        let result =
            OAuthRuntime::new().expect("runtime").request(request).await;
        assert!(matches!(result, Err(AuthorizationError::Temporary)));
        server.join().expect("server");
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    async fn stalled_browser_body_times_out_and_aborts_fetch() {
        use wasm_bindgen::JsValue;
        let options = web_sys::RequestInit::new();
        let abort = FetchAbort::new(&options).expect("abort controller");
        let signal = js_sys::Reflect::get(
            options.as_ref(),
            &JsValue::from_str("signal"),
        )
        .expect("signal");
        let response = js_sys::Function::new_no_args(
            "return new Response(new ReadableStream({ start() {} }));",
        )
        .call0(&JsValue::UNDEFINED)
        .expect("response")
        .dyn_into::<web_sys::Response>()
        .expect("response type");
        let result = request_with_timeout(
            async move {
                let _abort = abort;
                browser_body(response, 4).await?;
                Ok(oauth2::http::Response::new(Vec::new()))
            },
            Duration::from_millis(1),
        )
        .await;
        assert!(matches!(result, Err(AuthorizationError::Temporary)));
        assert_eq!(
            js_sys::Reflect::get(&signal, &JsValue::from_str("aborted"))
                .expect("aborted")
                .as_bool(),
            Some(true)
        );
    }

    #[wasm_bindgen_test]
    async fn oversized_browser_stream_is_cancelled_before_buffering_it() {
        let response = js_sys::Function::new_no_args(
            "return new Response(new ReadableStream({
                start(c) { c.enqueue(new Uint8Array(20)); c.close(); }
            }));",
        )
        .call0(&wasm_bindgen::JsValue::UNDEFINED)
        .expect("response")
        .dyn_into::<web_sys::Response>()
        .expect("response type");
        assert!(matches!(
            browser_body(response, 4).await,
            Err(AuthorizationError::Temporary)
        ));
    }
}
