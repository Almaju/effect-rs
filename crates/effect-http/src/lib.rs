//! An Effect-typed HTTP client.
//!
//! - [`HttpClient`] is the service trait. Tests swap in
//!   [`FakeHttpClient`]; production uses [`LiveHttpClient`] (built on
//!   `reqwest` with `rustls-tls`).
//! - The builder ([`get`], [`post`], [`put`], [`patch`], [`delete`])
//!   yields a [`RequestBuilder`]; `.send::<R>()` turns it into an
//!   `Effect<Response, HttpError, R: HttpClient>`.
//!
//! ```no_run
//! use effect::Effect;
//! use effect_http::{get, HttpClient, LiveHttpClient};
//!
//! # #[tokio::main] async fn main() {
//! let program = get("https://httpbin.org/json")
//!     .header("Accept", "application/json")
//!     .send::<LiveHttpClient>();
//!
//! let exit = program.run_with(LiveHttpClient::new()).await;
//! match exit {
//!     effect::Exit::Success(resp) => println!("status: {}", resp.status),
//!     effect::Exit::Failure(c) => eprintln!("error: {c}"),
//! }
//! # }
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use effect::Effect;
use thiserror::Error;

pub type AsyncResult<T> = Pin<Box<dyn Future<Output = T> + Send>>;

// ── Method ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
        }
    }
}

// ── Request / Response ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
    pub fn is_redirect(&self) -> bool {
        (300..400).contains(&self.status)
    }
    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status)
    }
    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status)
    }

    /// Consume the response body as a UTF-8 string. Returns
    /// `HttpError::InvalidBody` on non-UTF8 bytes.
    pub fn body_string(self) -> Result<String, HttpError> {
        String::from_utf8(self.body)
            .map_err(|e| HttpError::InvalidBody(format!("non-UTF8 body: {e}")))
    }
}

#[derive(Debug, Clone, Error)]
pub enum HttpError {
    #[error("network error: {0}")]
    Network(String),
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("invalid body: {0}")]
    InvalidBody(String),
    #[error("timeout")]
    Timeout,
    #[error("non-success status: {status}")]
    NonSuccessStatus { status: u16, body: Vec<u8> },
}

// ── Service trait + Live impl ───────────────────────────────────

pub trait HttpClient: Send + Sync + 'static {
    fn send(&self, req: Request) -> AsyncResult<Result<Response, HttpError>>;
}

/// `Arc<H>` is itself an `HttpClient` — convenient when you want to
/// keep a clone-able handle to a fake while the provider takes
/// ownership of the client.
impl<H: HttpClient> HttpClient for std::sync::Arc<H> {
    fn send(&self, req: Request) -> AsyncResult<Result<Response, HttpError>> {
        (**self).send(req)
    }
}

/// `reqwest`-backed implementation.
pub struct LiveHttpClient {
    inner: reqwest::Client,
}

impl LiveHttpClient {
    pub fn new() -> Self {
        LiveHttpClient {
            inner: reqwest::Client::new(),
        }
    }

    pub fn with_client(client: reqwest::Client) -> Self {
        LiveHttpClient { inner: client }
    }
}

impl Default for LiveHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient for LiveHttpClient {
    fn send(&self, req: Request) -> AsyncResult<Result<Response, HttpError>> {
        let client = self.inner.clone();
        Box::pin(async move {
            let url = reqwest::Url::parse(&req.url)
                .map_err(|e| HttpError::InvalidUrl(e.to_string()))?;
            let method = match req.method {
                Method::Get => reqwest::Method::GET,
                Method::Post => reqwest::Method::POST,
                Method::Put => reqwest::Method::PUT,
                Method::Patch => reqwest::Method::PATCH,
                Method::Delete => reqwest::Method::DELETE,
                Method::Head => reqwest::Method::HEAD,
                Method::Options => reqwest::Method::OPTIONS,
            };

            let mut builder = client.request(method, url);
            for (k, v) in req.headers {
                builder = builder.header(k, v);
            }
            if let Some(body) = req.body {
                builder = builder.body(body);
            }

            let response = builder
                .send()
                .await
                .map_err(|e| HttpError::Network(e.to_string()))?;
            let status = response.status().as_u16();
            let mut headers = HashMap::new();
            for (k, v) in response.headers() {
                if let Ok(s) = v.to_str() {
                    headers.insert(k.as_str().to_string(), s.to_string());
                }
            }
            let body = response
                .bytes()
                .await
                .map_err(|e| HttpError::Network(e.to_string()))?
                .to_vec();
            Ok(Response { status, headers, body })
        })
    }
}

// ── Builder ──────────────────────────────────────────────────────

pub struct RequestBuilder {
    method: Method,
    url: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
}

impl RequestBuilder {
    fn new(method: Method, url: impl Into<String>) -> Self {
        RequestBuilder {
            method,
            url: url.into(),
            headers: HashMap::new(),
            body: None,
        }
    }

    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    pub fn body(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.body = Some(bytes.into());
        self
    }

    /// Set the body and add `Content-Type: application/json`.
    pub fn json_body(mut self, json: impl Into<Vec<u8>>) -> Self {
        self.headers
            .insert("Content-Type".to_string(), "application/json".to_string());
        self.body = Some(json.into());
        self
    }

    pub fn build(self) -> Request {
        Request {
            method: self.method,
            url: self.url,
            headers: self.headers,
            body: self.body,
        }
    }

    /// Turn the built request into an `Effect<Response, HttpError, R>`.
    pub fn send<R: HttpClient>(self) -> Effect<Response, HttpError, R> {
        let req = self.build();
        Effect::from_fn(move |r: Arc<R>| {
            let req = req.clone();
            async move { r.send(req).await }
        })
    }

    /// `.send()` then assert HTTP 2xx; otherwise `Cause::Fail(NonSuccessStatus)`.
    pub fn send_ok<R: HttpClient>(self) -> Effect<Response, HttpError, R> {
        self.send().flat_map(|resp| {
            if resp.is_success() {
                Effect::<Response, HttpError, R>::sync(move || Ok(resp.clone()))
            } else {
                Effect::<Response, HttpError, R>::fail(HttpError::NonSuccessStatus {
                    status: resp.status,
                    body: resp.body,
                })
            }
        })
    }
}

pub fn get(url: impl Into<String>) -> RequestBuilder {
    RequestBuilder::new(Method::Get, url)
}
pub fn post(url: impl Into<String>) -> RequestBuilder {
    RequestBuilder::new(Method::Post, url)
}
pub fn put(url: impl Into<String>) -> RequestBuilder {
    RequestBuilder::new(Method::Put, url)
}
pub fn patch(url: impl Into<String>) -> RequestBuilder {
    RequestBuilder::new(Method::Patch, url)
}
pub fn delete(url: impl Into<String>) -> RequestBuilder {
    RequestBuilder::new(Method::Delete, url)
}

// ── FakeHttpClient — testing utility ────────────────────────────

use std::sync::Mutex;

/// A canned-response client for tests. Matches by exact URL.
pub struct FakeHttpClient {
    inner: Mutex<FakeInner>,
}

struct FakeInner {
    responses: HashMap<String, Result<Response, HttpError>>,
    recorded: Vec<Request>,
}

impl FakeHttpClient {
    pub fn new() -> Self {
        FakeHttpClient {
            inner: Mutex::new(FakeInner {
                responses: HashMap::new(),
                recorded: Vec::new(),
            }),
        }
    }

    pub fn expect(&self, url: impl Into<String>, response: Response) {
        self.inner
            .lock()
            .unwrap()
            .responses
            .insert(url.into(), Ok(response));
    }

    pub fn expect_error(&self, url: impl Into<String>, err: HttpError) {
        self.inner
            .lock()
            .unwrap()
            .responses
            .insert(url.into(), Err(err));
    }

    /// Every request the client has been asked to send, in order.
    pub fn recorded_requests(&self) -> Vec<Request> {
        self.inner.lock().unwrap().recorded.clone()
    }
}

impl Default for FakeHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient for FakeHttpClient {
    fn send(&self, req: Request) -> AsyncResult<Result<Response, HttpError>> {
        let mut guard = self.inner.lock().unwrap();
        guard.recorded.push(req.clone());
        let result = guard.responses.get(&req.url).cloned().ok_or_else(|| {
            HttpError::Network(format!("no canned response for {}", req.url))
        });
        let result = match result {
            Ok(stored) => stored,
            Err(e) => Err(e),
        };
        Box::pin(async move { result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_response(body: &str) -> Response {
        Response {
            status: 200,
            headers: HashMap::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    #[tokio::test]
    async fn get_returns_canned_response() {
        let client = FakeHttpClient::new();
        client.expect("https://example.com/data", ok_response("hello"));

        let exit = get("https://example.com/data")
            .send::<FakeHttpClient>()
            .run_with(client)
            .await;

        let resp = exit.ok().unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body_string().unwrap(), "hello");
    }

    #[tokio::test]
    async fn post_with_body_records_request() {
        let client = FakeHttpClient::new();
        client.expect(
            "https://example.com/items",
            Response {
                status: 201,
                headers: HashMap::new(),
                body: br#"{"id":1}"#.to_vec(),
            },
        );

        let arc = Arc::new(client);
        let exit = post("https://example.com/items")
            .json_body(br#"{"name":"alice"}"#.to_vec())
            .send::<FakeHttpClient>()
            .run(arc.clone())
            .await;

        let resp = exit.ok().unwrap();
        assert_eq!(resp.status, 201);

        let recorded = arc.recorded_requests();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].method, Method::Post);
        assert_eq!(
            recorded[0].headers.get("Content-Type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(recorded[0].body.as_ref().unwrap(), br#"{"name":"alice"}"#);
    }

    #[tokio::test]
    async fn header_is_passed_to_client() {
        let client = FakeHttpClient::new();
        client.expect("https://x.test/", ok_response(""));

        let arc = Arc::new(client);
        let _ = get("https://x.test/")
            .header("Authorization", "Bearer xyz")
            .header("X-Foo", "bar")
            .send::<FakeHttpClient>()
            .run(arc.clone())
            .await;

        let req = &arc.recorded_requests()[0];
        assert_eq!(
            req.headers.get("Authorization"),
            Some(&"Bearer xyz".to_string())
        );
        assert_eq!(req.headers.get("X-Foo"), Some(&"bar".to_string()));
    }

    #[tokio::test]
    async fn missing_canned_response_produces_network_error() {
        let client = FakeHttpClient::new();
        let exit = get("https://nope.test/")
            .send::<FakeHttpClient>()
            .run_with(client)
            .await;
        match exit.err() {
            Some(HttpError::Network(msg)) => assert!(msg.contains("no canned response")),
            other => panic!("expected Network error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn canned_error_propagates() {
        let client = FakeHttpClient::new();
        client.expect_error("https://x.test/", HttpError::Timeout);
        let exit = get("https://x.test/")
            .send::<FakeHttpClient>()
            .run_with(client)
            .await;
        assert!(matches!(exit.err(), Some(HttpError::Timeout)));
    }

    #[tokio::test]
    async fn send_ok_passes_2xx() {
        let client = FakeHttpClient::new();
        client.expect("https://x.test/ok", ok_response("hi"));
        let exit = get("https://x.test/ok")
            .send_ok::<FakeHttpClient>()
            .run_with(client)
            .await;
        let resp = exit.ok().unwrap();
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn send_ok_fails_on_4xx() {
        let client = FakeHttpClient::new();
        client.expect(
            "https://x.test/missing",
            Response {
                status: 404,
                headers: HashMap::new(),
                body: b"not found".to_vec(),
            },
        );
        let exit = get("https://x.test/missing")
            .send_ok::<FakeHttpClient>()
            .run_with(client)
            .await;
        match exit.err() {
            Some(HttpError::NonSuccessStatus { status, body }) => {
                assert_eq!(status, 404);
                assert_eq!(body, b"not found");
            }
            other => panic!("expected NonSuccessStatus 404, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn response_classifiers_match_ranges() {
        let r = Response { status: 200, headers: HashMap::new(), body: vec![] };
        assert!(r.is_success());
        assert!(!r.is_redirect());
        let r = Response { status: 302, headers: HashMap::new(), body: vec![] };
        assert!(r.is_redirect());
        let r = Response { status: 404, headers: HashMap::new(), body: vec![] };
        assert!(r.is_client_error());
        let r = Response { status: 503, headers: HashMap::new(), body: vec![] };
        assert!(r.is_server_error());
    }
}
