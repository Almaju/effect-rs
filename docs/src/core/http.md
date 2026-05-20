# HTTP Client

`effect-http` is an Effect-typed HTTP client. The `HttpClient` trait
makes it trivial to mock in tests; `LiveHttpClient` is a thin wrapper
around `reqwest` (with `rustls-tls`) for production.

```toml
[dependencies]
effect      = { version = "0.0.1" }
effect-http = { version = "0.0.1" }
```

## A first request

```rust,no_run
use effect_http::{get, LiveHttpClient};

# #[tokio::main] async fn main() {
let exit = get("https://httpbin.org/json")
    .header("Accept", "application/json")
    .send::<LiveHttpClient>()
    .run_with(LiveHttpClient::new())
    .await;

match exit {
    effect::Exit::Success(resp) => println!("status: {}", resp.status),
    effect::Exit::Failure(c)    => eprintln!("error: {c}"),
}
# }
```

## Builders

| Function     | Method            |
| ------------ | ----------------- |
| `get(url)`   | `GET`             |
| `post(url)`  | `POST`            |
| `put(url)`   | `PUT`             |
| `patch(url)` | `PATCH`           |
| `delete(url)`| `DELETE`          |

Each returns a `RequestBuilder` with fluent setters:

```rust,no_run
# use effect_http::{post, FakeHttpClient};
# #[tokio::main] async fn main() {
let _ = post("https://api.example.com/items")
    .header("Authorization", "Bearer xyz")
    .json_body(br#"{"name":"alice"}"#.to_vec())  // sets Content-Type
    .send::<FakeHttpClient>();
# }
```

| Builder method            | Effect                                                  |
| ------------------------- | ------------------------------------------------------- |
| `.header(k, v)`           | add a request header                                    |
| `.body(bytes)`            | raw bytes                                               |
| `.json_body(bytes)`       | bytes + `Content-Type: application/json`                |
| `.build()`                | finalize without sending                                |
| `.send::<R>()`            | run as `Effect<Response, HttpError, R: HttpClient>`     |
| `.send_ok::<R>()`         | same, but `Cause::Fail(NonSuccessStatus)` on non-2xx    |

## `Response`

```rust,no_run
# use effect_http::Response;
# let resp = Response { status: 200, headers: Default::default(), body: vec![] };
let _ = resp.is_success();         // 2xx
let _ = resp.is_redirect();        // 3xx
let _ = resp.is_client_error();    // 4xx
let _ = resp.is_server_error();    // 5xx
# let resp = Response { status: 200, headers: Default::default(), body: b"hi".to_vec() };
let body: String = resp.body_string().unwrap();
```

## Errors

```rust,ignore
pub enum HttpError {
    Network(String),
    InvalidUrl(String),
    InvalidBody(String),
    Timeout,
    NonSuccessStatus { status: u16, body: Vec<u8> },
}
```

Returned in the `Effect`'s `E` channel — handle with `catch_all`,
`map_error`, etc.

## Testing — `FakeHttpClient`

```rust,no_run
use effect_http::{get, FakeHttpClient, Response, HttpClient};
use std::sync::Arc;

# #[tokio::main] async fn main() {
let client = FakeHttpClient::new();
client.expect(
    "https://api.test/data",
    Response { status: 200, headers: Default::default(), body: b"hi".to_vec() },
);
let arc = Arc::new(client);

let exit = get("https://api.test/data")
    .send::<FakeHttpClient>()
    .run(arc.clone())
    .await;
assert!(exit.cause().is_none());

// Inspect what the client was asked to send.
let recorded = arc.recorded_requests();
assert_eq!(recorded.len(), 1);
assert_eq!(recorded[0].method, effect_http::Method::Get);
# }
```

The fake records every request, so you can assert on URLs, methods,
headers, and bodies without standing up a test HTTP server.

## What's coming

- **`json::<T: Schema>(resp)` decoder** — parse the body via Schema in
  one step, surface `SchemaError` with field-path context.
- **`expect_pattern(re, response)`** — regex matchers on the fake for
  more flexible test scenarios.
- **HTTP server** (`effect-http-server` or similar) — typed handlers
  backed by hyper / axum.
- **HttpApi** — schema-first endpoint declarations that generate both
  server router and typed client.
- **Retry / timeout middleware** — `Effect::retry(http_schedule)`
  paired with `.timeout(d)` once the latter lands.

## Why reqwest + rustls?

- `reqwest` is the de facto Rust HTTP client; covers TLS, redirects,
  cookies, multipart out of the box.
- `rustls-tls` avoids depending on the system's OpenSSL — same binary
  works on Alpine, distroless, etc.
- The dep is heavyweight (~30 transitive crates). If you have stricter
  needs, plug in your own `HttpClient` impl — the trait is small.
