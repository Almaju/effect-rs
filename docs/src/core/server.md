# HTTP Server

`effect-http-server` is a small HTTP server built on `hyper` 1.x. You
build a [`Router`] declaratively, hand it to [`serve`], and let it run
on a [`tokio::net::TcpListener`] of your choosing.

```toml
[dependencies]
effect-http-server = { version = "0.0.1" }
tokio              = { version = "1", features = ["full"] }
```

## A minimal server

```rust,no_run
use effect_http_server::*;
use tokio::net::TcpListener;

# #[tokio::main] async fn main() -> Result<(), ServerError> {
let router = Router::new()
    .get("/hello", |_req: Request| async move {
        Ok(Response::text(200, "hi there"))
    })
    .post("/echo", |req: Request| async move {
        Ok(Response {
            status: 200,
            headers: Default::default(),
            body: req.body,
        })
    });

let listener = TcpListener::bind("127.0.0.1:3000").await
    .map_err(|e| ServerError::Io(e.to_string()))?;

serve(listener, router).await
# }
```

## Router

| Builder method                         | Effect                                       |
| -------------------------------------- | -------------------------------------------- |
| `Router::new()`                        | empty                                        |
| `.get(path, h)`, `.post(path, h)`, … | register a method+path → handler             |
| `.route(method, path, h)`              | underlying form                              |
| `.fallback(h)`                         | replace the default 404                      |

Matching is exact on `(method, path)` for now; pattern paths
(`/users/:id`) are planned.

## Handler

```rust,ignore
pub trait Handler: Send + Sync + 'static {
    fn handle(&self, req: Request)
        -> AsyncResult<Result<Response, ServerError>>;
}
```

Any `Fn(Request) -> Future<Output = Result<Response, ServerError>>`
is a `Handler` via a blanket impl, so handlers are usually written
inline:

```rust,no_run
# use effect_http_server::*;
let _: Box<dyn Handler> = Box::new(|req: Request| async move {
    Ok(Response::text(200, format!("you sent {} bytes", req.body.len())))
});
```

## Request / Response

```rust,ignore
pub struct Request {
    pub method: effect_http::Method,   // re-used from the client crate
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

pub struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}
```

Convenience constructors:

| Constructor                    | Sets                                           |
| ------------------------------ | ---------------------------------------------- |
| `Response::new(status)`        | empty body, empty headers                      |
| `Response::text(status, body)` | `text/plain` + UTF-8 body                       |
| `Response::json(status, bytes)`| `application/json` + body                       |
| `Response::not_found()`        | 404 plain text                                  |
| `Response::internal_error(msg)`| 500 plain text                                  |

Plus `.header(k, v)` to add headers fluently.

## Errors

```rust,ignore
pub enum ServerError {
    Io(String),
    Handler(String),
    Body(String),
}
```

A handler returning `Err(ServerError::...)` becomes a 500 response
with the error message as the body. Bind errors (`Io`) only surface
from the top-level `serve` return.

## Sharing state

The Handler trait is `Send + Sync + 'static`, so handlers can close
over `Arc<T>` for shared state:

```rust,no_run
use effect_http_server::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

# fn main() {
let counter = Arc::new(AtomicUsize::new(0));

let counter_clone = counter.clone();
let router = Router::new().get("/inc", move |_req: Request| {
    let c = counter_clone.clone();
    async move {
        let n = c.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(Response::text(200, format!("count: {n}")))
    }
});
# let _ = router;
# }
```

For Effect-typed handlers, just wrap the run inside the async block:

```rust,no_run
use effect::Effect;
use effect_http_server::*;
use std::sync::Arc;

# pub struct Db;
# fn fetch_user(_: Arc<Db>, _: &str) -> Effect<String, String, ()> {
#     Effect::succeed("alice".into())
# }
# fn main() {
let db = Arc::new(Db);
let _ = Router::new().get("/users/me", move |_req: Request| {
    let db = db.clone();
    async move {
        let exit = fetch_user(db, "me").execute().await;
        match exit.ok() {
            Some(name) => Ok(Response::text(200, name)),
            None => Ok(Response::internal_error("lookup failed")),
        }
    }
});
# }
```

## What's coming

- **Path patterns** — `/users/:id` with extracted captures in a
  `Request::params` map.
- **Query params** — parsed lazily into a `HashMap`.
- **Schema-typed handlers** — `handler.json::<Req, Resp>(|req: Req| async ...)`
  that wraps schema parse/encode + 400-on-bad-request.
- **Effect-typed wrapper** — `effect_handler(ctx, |req| -> Effect<...>)`
  so handlers can compose with the full Effect stack without manual
  glue.
- **Graceful shutdown** — `serve_with_shutdown(listener, router, signal)`.
- **TLS** — `rustls`-based listener wrapper.
- **HTTP/2 + WebSocket** — when needed.
