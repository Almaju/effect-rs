//! Minimal HTTP server built on `hyper` 1.x.
//!
//! Build a [`Router`] declaratively, hand it to [`serve`], let it run
//! on a [`tokio::net::TcpListener`] of your choosing.
//!
//! ```no_run
//! use effect_http_server::*;
//! use tokio::net::TcpListener;
//!
//! # #[tokio::main] async fn main() -> Result<(), ServerError> {
//! let router = Router::new()
//!     .get("/hello", |_req: Request| async move {
//!         Ok(Response::text(200, "hi there"))
//!     });
//!
//! let listener = TcpListener::bind("127.0.0.1:0").await
//!     .map_err(|e| ServerError::Io(e.to_string()))?;
//! serve(listener, router).await
//! # }
//! ```
//!
//! Handlers are any `Fn(Request) -> Future<Output = Result<Response, ServerError>>`,
//! which means you can wrap a typed `Effect` chain inline.

use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use effect_http::Method;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use thiserror::Error;
use tokio::net::TcpListener;

pub type AsyncResult<T> = Pin<Box<dyn Future<Output = T> + Send>>;

// ── Errors ───────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("I/O error: {0}")]
    Io(String),

    #[error("handler error: {0}")]
    Handler(String),

    #[error("body too large or invalid: {0}")]
    Body(String),
}

// ── Request / Response ───────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16) -> Self {
        Response {
            status,
            headers: HashMap::new(),
            body: Vec::new(),
        }
    }

    pub fn text(status: u16, body: impl Into<String>) -> Self {
        let body = body.into().into_bytes();
        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        );
        Response { status, headers, body }
    }

    pub fn json(status: u16, json: impl Into<Vec<u8>>) -> Self {
        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            "application/json".to_string(),
        );
        Response {
            status,
            headers,
            body: json.into(),
        }
    }

    pub fn not_found() -> Self {
        Response::text(404, "not found")
    }

    pub fn internal_error(msg: impl Into<String>) -> Self {
        Response::text(500, msg)
    }

    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }
}

// ── Handler ──────────────────────────────────────────────────────

pub trait Handler: Send + Sync + 'static {
    fn handle(&self, req: Request) -> AsyncResult<Result<Response, ServerError>>;
}

impl<F, Fut> Handler for F
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, ServerError>> + Send + 'static,
{
    fn handle(&self, req: Request) -> AsyncResult<Result<Response, ServerError>> {
        Box::pin(self(req))
    }
}

// ── Router ───────────────────────────────────────────────────────

pub struct Router {
    routes: Vec<(Method, String, Box<dyn Handler>)>,
    fallback: Option<Box<dyn Handler>>,
}

impl Router {
    pub fn new() -> Self {
        Router {
            routes: Vec::new(),
            fallback: None,
        }
    }

    pub fn route(mut self, method: Method, path: impl Into<String>, h: impl Handler) -> Self {
        self.routes.push((method, path.into(), Box::new(h)));
        self
    }

    pub fn get(self, path: impl Into<String>, h: impl Handler) -> Self {
        self.route(Method::Get, path, h)
    }
    pub fn post(self, path: impl Into<String>, h: impl Handler) -> Self {
        self.route(Method::Post, path, h)
    }
    pub fn put(self, path: impl Into<String>, h: impl Handler) -> Self {
        self.route(Method::Put, path, h)
    }
    pub fn patch(self, path: impl Into<String>, h: impl Handler) -> Self {
        self.route(Method::Patch, path, h)
    }
    pub fn delete(self, path: impl Into<String>, h: impl Handler) -> Self {
        self.route(Method::Delete, path, h)
    }

    /// Set the fallback handler invoked when no route matches. Defaults
    /// to a 404 plain-text response.
    pub fn fallback(mut self, h: impl Handler) -> Self {
        self.fallback = Some(Box::new(h));
        self
    }

    async fn dispatch(&self, req: Request) -> Response {
        for (m, p, h) in &self.routes {
            if *m == req.method && *p == req.path {
                return match h.handle(req).await {
                    Ok(r) => r,
                    Err(e) => Response::internal_error(format!("{e}")),
                };
            }
        }
        if let Some(h) = &self.fallback {
            match h.handle(req).await {
                Ok(r) => r,
                Err(e) => Response::internal_error(format!("{e}")),
            }
        } else {
            Response::not_found()
        }
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

// ── serve ────────────────────────────────────────────────────────

/// Run `router` over connections accepted from `listener`. Returns
/// only on an unrecoverable accept error; individual connection
/// errors are logged via `eprintln!` and otherwise ignored.
///
/// For graceful shutdown, drop the listener.
pub async fn serve(listener: TcpListener, router: Router) -> Result<(), ServerError> {
    let router = Arc::new(router);
    loop {
        let (stream, _peer) = match listener.accept().await {
            Ok(s) => s,
            Err(e) => return Err(ServerError::Io(format!("accept: {e}"))),
        };
        let router = router.clone();
        let io = TokioIo::new(stream);
        tokio::spawn(async move {
            let service = service_fn(move |hyper_req: hyper::Request<Incoming>| {
                let router = router.clone();
                async move {
                    let req = match from_hyper(hyper_req).await {
                        Ok(r) => r,
                        Err(_) => {
                            return Ok::<_, Infallible>(
                                to_hyper(Response::internal_error("bad request body")),
                            );
                        }
                    };
                    let resp = router.dispatch(req).await;
                    Ok::<_, Infallible>(to_hyper(resp))
                }
            });
            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                eprintln!("[effect-http-server] connection error: {err}");
            }
        });
    }
}

async fn from_hyper(req: hyper::Request<Incoming>) -> Result<Request, ServerError> {
    let (parts, body) = req.into_parts();
    let bytes = body
        .collect()
        .await
        .map_err(|e| ServerError::Body(e.to_string()))?
        .to_bytes()
        .to_vec();
    let method = match parts.method {
        hyper::Method::GET => Method::Get,
        hyper::Method::POST => Method::Post,
        hyper::Method::PUT => Method::Put,
        hyper::Method::PATCH => Method::Patch,
        hyper::Method::DELETE => Method::Delete,
        hyper::Method::HEAD => Method::Head,
        hyper::Method::OPTIONS => Method::Options,
        other => return Err(ServerError::Body(format!("unsupported method: {other}"))),
    };
    let mut headers = HashMap::new();
    for (k, v) in &parts.headers {
        if let Ok(s) = v.to_str() {
            headers.insert(k.as_str().to_string(), s.to_string());
        }
    }
    let path = parts.uri.path().to_string();
    Ok(Request {
        method,
        path,
        headers,
        body: bytes,
    })
}

fn to_hyper(resp: Response) -> hyper::Response<Full<Bytes>> {
    let mut builder = hyper::Response::builder().status(resp.status);
    for (k, v) in resp.headers {
        builder = builder.header(k, v);
    }
    builder
        .body(Full::new(Bytes::from(resp.body)))
        .expect("response build failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    async fn spawn_server(router: Router) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = serve(listener, router).await;
        });
        // Give the server a tick to be ready.
        tokio::time::sleep(Duration::from_millis(20)).await;
        addr
    }

    async fn http_get(addr: std::net::SocketAddr, path: &str) -> (u16, String) {
        let url = format!("http://{}{}", addr, path);
        let resp = reqwest::get(&url).await.expect("request");
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        (status, body)
    }

    async fn http_post(addr: std::net::SocketAddr, path: &str, body: &[u8]) -> (u16, String) {
        let url = format!("http://{}{}", addr, path);
        let client = reqwest::Client::new();
        let resp = client.post(&url).body(body.to_vec()).send().await.expect("post");
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        (status, body)
    }

    #[tokio::test]
    async fn get_returns_text_response() {
        let router = Router::new().get("/hello", |_req: Request| async move {
            Ok(Response::text(200, "hi"))
        });
        let addr = spawn_server(router).await;
        let (status, body) = http_get(addr, "/hello").await;
        assert_eq!(status, 200);
        assert_eq!(body, "hi");
    }

    #[tokio::test]
    async fn unknown_path_returns_404() {
        let router = Router::new().get("/registered", |_req: Request| async move {
            Ok(Response::text(200, "ok"))
        });
        let addr = spawn_server(router).await;
        let (status, _) = http_get(addr, "/elsewhere").await;
        assert_eq!(status, 404);
    }

    #[tokio::test]
    async fn fallback_handler_replaces_default_404() {
        let router = Router::new().fallback(|_req: Request| async move {
            Ok(Response::text(418, "i'm a teapot"))
        });
        let addr = spawn_server(router).await;
        let (status, body) = http_get(addr, "/anything").await;
        assert_eq!(status, 418);
        assert_eq!(body, "i'm a teapot");
    }

    #[tokio::test]
    async fn post_receives_body() {
        let router = Router::new().post("/echo", |req: Request| async move {
            let body = req.body.clone();
            Ok(Response {
                status: 200,
                headers: HashMap::new(),
                body,
            })
        });
        let addr = spawn_server(router).await;
        let (status, body) = http_post(addr, "/echo", b"hello world").await;
        assert_eq!(status, 200);
        assert_eq!(body, "hello world");
    }

    #[tokio::test]
    async fn handler_error_surfaces_as_500() {
        let router = Router::new().get("/bad", |_req: Request| async move {
            Err::<Response, _>(ServerError::Handler("oops".into()))
        });
        let addr = spawn_server(router).await;
        let (status, body) = http_get(addr, "/bad").await;
        assert_eq!(status, 500);
        assert!(body.contains("oops"));
    }

    #[tokio::test]
    async fn shared_state_via_arc_works_inside_handler() {
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let router = Router::new().get("/inc", move |_req: Request| {
            let c = counter_clone.clone();
            async move {
                c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(Response::text(200, "+1"))
            }
        });
        let addr = spawn_server(router).await;
        let _ = http_get(addr, "/inc").await;
        let _ = http_get(addr, "/inc").await;
        let _ = http_get(addr, "/inc").await;
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn json_response_sets_content_type() {
        let router = Router::new().get("/data", |_req: Request| async move {
            Ok(Response::json(200, br#"{"ok":true}"#.to_vec()))
        });
        let addr = spawn_server(router).await;
        let url = format!("http://{}/data", addr);
        let resp = reqwest::get(&url).await.unwrap();
        let ct = resp.headers().get("content-type").map(|v| v.to_str().unwrap().to_string());
        assert_eq!(ct.as_deref(), Some("application/json"));
        let body = resp.text().await.unwrap();
        assert_eq!(body, r#"{"ok":true}"#);
    }
}
