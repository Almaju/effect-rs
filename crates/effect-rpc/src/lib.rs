//! Typed RPC over a pluggable transport.
//!
//! An [`Endpoint<Req, Resp>`] is a static description of a remote
//! procedure: a path, a method, and the request/response types it
//! expects (both `Schema`). [`call`] threads encode → transport →
//! decode through an `Effect`. [`RpcTransport`] is the abstract
//! service; [`LiveHttpRpcTransport`] is a default backed by
//! [`effect_http`].
//!
//! ```ignore
//! use effect::Schema;
//! use effect_rpc::*;
//! use effect_http::Method;
//!
//! #[derive(Schema, Clone)]
//! struct GetUser { id: u64 }
//!
//! #[derive(Schema, Clone)]
//! struct User { id: u64, name: String }
//!
//! let get_user: Endpoint<GetUser, User> =
//!     Endpoint::new("/users/get", Method::Post);
//!
//! // Run it:
//! let user = call(get_user, GetUser { id: 42 })
//!     .run_with(LiveHttpRpcTransport::new("https://api.example.com", http_client))
//!     .await;
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use effect::{Effect, Schema, SchemaError};
use effect_http::{HttpClient, HttpError, Method, Request, Response};
use effect_schema::serde_json;
use thiserror::Error;

pub type AsyncResult<T> = Pin<Box<dyn Future<Output = T> + Send>>;

// ── Endpoint ─────────────────────────────────────────────────────

/// A static description of a remote procedure: where it lives + what
/// it takes / returns.
pub struct Endpoint<Req, Resp> {
    pub path: &'static str,
    pub method: Method,
    _phantom: PhantomData<fn(Req) -> Resp>,
}

impl<Req, Resp> Clone for Endpoint<Req, Resp> {
    fn clone(&self) -> Self {
        Endpoint {
            path: self.path,
            method: self.method,
            _phantom: PhantomData,
        }
    }
}

impl<Req, Resp> Copy for Endpoint<Req, Resp> {}

impl<Req, Resp> Endpoint<Req, Resp> {
    pub const fn new(path: &'static str, method: Method) -> Self {
        Endpoint { path, method, _phantom: PhantomData }
    }

    /// Shortcut for `Endpoint::new(path, Method::Post)`.
    pub const fn post(path: &'static str) -> Self {
        Endpoint::new(path, Method::Post)
    }

    /// Shortcut for `Endpoint::new(path, Method::Get)`.
    pub const fn get(path: &'static str) -> Self {
        Endpoint::new(path, Method::Get)
    }
}

// ── Errors ───────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("transport error: {0}")]
    Transport(String),

    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },

    #[error("request encode error: {0}")]
    EncodeRequest(String),

    #[error("response decode error: {0}")]
    DecodeResponse(#[source] SchemaError),
}

impl Clone for RpcError {
    fn clone(&self) -> Self {
        match self {
            RpcError::Transport(s) => RpcError::Transport(s.clone()),
            RpcError::Http { status, body } => RpcError::Http {
                status: *status,
                body: body.clone(),
            },
            RpcError::EncodeRequest(s) => RpcError::EncodeRequest(s.clone()),
            // SchemaError isn't Clone; render the message.
            RpcError::DecodeResponse(e) => {
                RpcError::EncodeRequest(format!("(re-cloned) decode error: {e}"))
            }
        }
    }
}

// ── Transport trait ─────────────────────────────────────────────

/// A byte-pipe over which an `Endpoint` invocation is dispatched.
/// Implementations dispatch on `path` and `method` and return the
/// raw response body.
pub trait RpcTransport: Send + Sync + 'static {
    fn send(
        &self,
        path: &'static str,
        method: Method,
        request_body: Vec<u8>,
    ) -> AsyncResult<Result<Vec<u8>, RpcError>>;
}

// ── Live HTTP transport ─────────────────────────────────────────

/// An `RpcTransport` backed by an [`HttpClient`]. POSTs the encoded
/// request body to `<base_url><path>` (or GET/PUT/etc per the
/// `Endpoint::method`); a 2xx response yields its body, non-2xx
/// becomes [`RpcError::Http`].
pub struct LiveHttpRpcTransport<H: HttpClient> {
    base_url: String,
    http: Arc<H>,
    headers: HashMap<String, String>,
}

impl<H: HttpClient> LiveHttpRpcTransport<H> {
    pub fn new(base_url: impl Into<String>, http: H) -> Self {
        LiveHttpRpcTransport {
            base_url: base_url.into(),
            http: Arc::new(http),
            headers: HashMap::new(),
        }
    }

    pub fn with_header(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.headers.insert(k.into(), v.into());
        self
    }
}

impl<H: HttpClient> RpcTransport for LiveHttpRpcTransport<H> {
    fn send(
        &self,
        path: &'static str,
        method: Method,
        request_body: Vec<u8>,
    ) -> AsyncResult<Result<Vec<u8>, RpcError>> {
        let url = format!("{}{}", self.base_url, path);
        let http = self.http.clone();
        let mut headers = self.headers.clone();
        headers
            .entry("Content-Type".to_string())
            .or_insert_with(|| "application/json".to_string());
        let req = Request {
            method,
            url,
            headers,
            body: Some(request_body),
        };
        Box::pin(async move {
            match http.send(req).await {
                Ok(Response { status, body, .. }) if (200..300).contains(&status) => Ok(body),
                Ok(Response { status, body, .. }) => Err(RpcError::Http {
                    status,
                    body: String::from_utf8_lossy(&body).to_string(),
                }),
                Err(HttpError::Timeout) => Err(RpcError::Transport("timeout".into())),
                Err(e) => Err(RpcError::Transport(e.to_string())),
            }
        })
    }
}

// ── call helper ─────────────────────────────────────────────────

/// Invoke `endpoint`: encode `request` via [`Schema`], send via the
/// context's [`RpcTransport`], decode the response via [`Schema`].
pub fn call<Req, Resp, R>(
    endpoint: Endpoint<Req, Resp>,
    request: Req,
) -> Effect<Resp, RpcError, R>
where
    Req: Schema + Clone + Send + Sync + 'static,
    Resp: Schema + Send + 'static,
    R: RpcTransport,
{
    Effect::from_fn(move |r: Arc<R>| {
        let request = request.clone();
        async move {
            let json = request.encode_json();
            let bytes = serde_json::to_vec(&json)
                .map_err(|e| RpcError::EncodeRequest(e.to_string()))?;
            let resp_bytes = r.send(endpoint.path, endpoint.method, bytes).await?;
            let resp_json: serde_json::Value =
                serde_json::from_slice(&resp_bytes).map_err(|e| {
                    RpcError::DecodeResponse(SchemaError::invalid(format!(
                        "response is not valid JSON: {e}"
                    )))
                })?;
            Resp::parse_json(&resp_json).map_err(RpcError::DecodeResponse)
        }
    })
}

// ── FakeRpcTransport ────────────────────────────────────────────

pub struct FakeRpcTransport {
    inner: Mutex<FakeInner>,
}

struct FakeInner {
    responses: HashMap<&'static str, Result<Vec<u8>, RpcError>>,
    recorded: Vec<(&'static str, Method, Vec<u8>)>,
}

impl FakeRpcTransport {
    pub fn new() -> Self {
        FakeRpcTransport {
            inner: Mutex::new(FakeInner {
                responses: HashMap::new(),
                recorded: Vec::new(),
            }),
        }
    }

    pub fn expect(&self, path: &'static str, body: Vec<u8>) {
        self.inner.lock().unwrap().responses.insert(path, Ok(body));
    }

    pub fn expect_error(&self, path: &'static str, err: RpcError) {
        self.inner.lock().unwrap().responses.insert(path, Err(err));
    }

    pub fn recorded(&self) -> Vec<(&'static str, Method, Vec<u8>)> {
        self.inner.lock().unwrap().recorded.clone()
    }
}

impl Default for FakeRpcTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl RpcTransport for FakeRpcTransport {
    fn send(
        &self,
        path: &'static str,
        method: Method,
        request_body: Vec<u8>,
    ) -> AsyncResult<Result<Vec<u8>, RpcError>> {
        let mut guard = self.inner.lock().unwrap();
        guard.recorded.push((path, method, request_body));
        let response = guard
            .responses
            .get(path)
            .cloned()
            .unwrap_or_else(|| Err(RpcError::Transport(format!("no canned response for {path}"))));
        Box::pin(async move { response })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use effect::Schema;
    use effect_http::Method;

    #[derive(Debug, Clone, PartialEq, Schema)]
    pub struct GetUser {
        pub id: u64,
    }

    #[derive(Debug, Clone, PartialEq, Schema)]
    pub struct User {
        pub id: u64,
        pub name: String,
    }

    const GET_USER: Endpoint<GetUser, User> = Endpoint::post("/users/get");

    #[tokio::test]
    async fn call_roundtrips_request_and_response() {
        let transport = FakeRpcTransport::new();
        // Pretend server response.
        let resp_bytes = serde_json::to_vec(&serde_json::json!({
            "id": 42, "name": "alice"
        }))
        .unwrap();
        transport.expect("/users/get", resp_bytes);
        let arc = Arc::new(transport);

        let exit = call::<GetUser, User, FakeRpcTransport>(
            GET_USER,
            GetUser { id: 42 },
        )
        .run(arc.clone())
        .await;

        let user = exit.ok().unwrap();
        assert_eq!(user, User { id: 42, name: "alice".into() });

        // Verify request was serialized correctly.
        let recorded = arc.recorded();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, "/users/get");
        assert_eq!(recorded[0].1, Method::Post);
        let req_json: serde_json::Value = serde_json::from_slice(&recorded[0].2).unwrap();
        assert_eq!(req_json["id"], 42);
    }

    #[tokio::test]
    async fn missing_canned_response_is_transport_error() {
        let transport = FakeRpcTransport::new();
        let exit = call::<GetUser, User, FakeRpcTransport>(
            GET_USER,
            GetUser { id: 1 },
        )
        .run_with(transport)
        .await;
        match exit.err() {
            Some(RpcError::Transport(msg)) => assert!(msg.contains("no canned response")),
            other => panic!("expected Transport error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn malformed_response_is_decode_error() {
        let transport = FakeRpcTransport::new();
        transport.expect("/users/get", b"not json at all".to_vec());
        let exit = call::<GetUser, User, FakeRpcTransport>(
            GET_USER,
            GetUser { id: 1 },
        )
        .run_with(transport)
        .await;
        match exit.err() {
            Some(RpcError::DecodeResponse(_)) => {}
            other => panic!("expected DecodeResponse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn response_with_missing_field_is_decode_error() {
        let transport = FakeRpcTransport::new();
        let bad = serde_json::to_vec(&serde_json::json!({ "id": 1 })).unwrap();
        transport.expect("/users/get", bad);
        let exit = call::<GetUser, User, FakeRpcTransport>(
            GET_USER,
            GetUser { id: 1 },
        )
        .run_with(transport)
        .await;
        match exit.err() {
            Some(RpcError::DecodeResponse(inner)) => {
                let msg = format!("{inner}");
                assert!(msg.contains("name") || msg.contains("missing"), "got: {msg}");
            }
            other => panic!("expected DecodeResponse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn live_http_transport_uses_supplied_client() {
        use effect_http::{FakeHttpClient, Response};
        let http = FakeHttpClient::new();
        let resp_bytes = serde_json::to_vec(&serde_json::json!({
            "id": 7, "name": "bob"
        }))
        .unwrap();
        http.expect(
            "https://api.test/users/get",
            Response {
                status: 200,
                headers: HashMap::new(),
                body: resp_bytes,
            },
        );
        let transport = LiveHttpRpcTransport::new("https://api.test", http);

        let exit = call::<GetUser, User, LiveHttpRpcTransport<FakeHttpClient>>(
            GET_USER,
            GetUser { id: 7 },
        )
        .run_with(transport)
        .await;

        let user = exit.ok().unwrap();
        assert_eq!(user, User { id: 7, name: "bob".into() });
    }

    #[tokio::test]
    async fn live_http_transport_surfaces_non_2xx_as_rpc_error() {
        use effect_http::{FakeHttpClient, Response};
        let http = FakeHttpClient::new();
        http.expect(
            "https://api.test/users/get",
            Response {
                status: 500,
                headers: HashMap::new(),
                body: b"server fell over".to_vec(),
            },
        );
        let transport = LiveHttpRpcTransport::new("https://api.test", http);

        let exit = call::<GetUser, User, LiveHttpRpcTransport<FakeHttpClient>>(
            GET_USER,
            GetUser { id: 1 },
        )
        .run_with(transport)
        .await;

        match exit.err() {
            Some(RpcError::Http { status, body }) => {
                assert_eq!(status, 500);
                assert_eq!(body, "server fell over");
            }
            other => panic!("expected Http error, got {other:?}"),
        }
    }
}
