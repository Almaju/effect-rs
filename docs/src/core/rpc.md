# RPC

`effect-rpc` is typed request/response over a pluggable transport.
`Endpoint<Req, Resp>` ties a path + method to the types it expects;
`call(endpoint, req)` encodes via [`Schema`](../data/schemas.md),
sends via the transport, decodes the response.

```toml
[dependencies]
effect        = { version = "0.0.1" }
effect-rpc    = { version = "0.0.1" }
effect-http   = { version = "0.0.1" }     # only if using the HTTP transport
```

## Define an endpoint

```rust,no_run
use effect::Schema;
use effect_rpc::Endpoint;

#[derive(Schema, Clone)]
pub struct GetUser { pub id: u64 }

#[derive(Schema, Clone)]
pub struct User { pub id: u64, pub name: String }

pub const GET_USER: Endpoint<GetUser, User> = Endpoint::post("/users/get");
```

`Endpoint::post` / `Endpoint::get` / `Endpoint::new(path, method)`
give you the shape. Both `Req` and `Resp` must implement `Schema`.

## Call it

```rust,no_run
use effect_rpc::{call, LiveHttpRpcTransport};
use effect_http::LiveHttpClient;

# use effect::Schema;
# #[derive(Schema, Clone)] pub struct GetUser { pub id: u64 }
# #[derive(Schema, Clone)] pub struct User { pub id: u64, pub name: String }
# const GET_USER: effect_rpc::Endpoint<GetUser, User> = effect_rpc::Endpoint::post("/users/get");
# #[tokio::main] async fn main() {
let transport = LiveHttpRpcTransport::new(
    "https://api.example.com",
    LiveHttpClient::new(),
);

let exit = call::<GetUser, User, _>(GET_USER, GetUser { id: 42 })
    .run_with(transport)
    .await;

match exit {
    effect::Exit::Success(user) => println!("got {}", user.name),
    effect::Exit::Failure(c)    => eprintln!("rpc failed: {c}"),
}
# }
```

## Errors

```rust,ignore
pub enum RpcError {
    Transport(String),                       // network / serde / generic
    Http { status: u16, body: String },      // non-2xx from HTTP transport
    EncodeRequest(String),                   // failed to serialize Req
    DecodeResponse(SchemaError),             // response didn't match Resp's Schema
}
```

`DecodeResponse` carries a structured [`SchemaError`] so failures
include the offending field path (e.g.
`at field 'name': type mismatch: expected string, got null`).

## Testing — `FakeRpcTransport`

```rust,no_run
use effect_rpc::{call, FakeRpcTransport, Endpoint};
use effect::Schema;
use effect_schema::serde_json;

# #[derive(Schema, Clone, PartialEq, Debug)] pub struct GetUser { pub id: u64 }
# #[derive(Schema, Clone, PartialEq, Debug)] pub struct User { pub id: u64, pub name: String }
# #[tokio::main] async fn main() {
const GET_USER: Endpoint<GetUser, User> = Endpoint::post("/users/get");

let transport = FakeRpcTransport::new();
let canned = serde_json::to_vec(&serde_json::json!({
    "id": 42, "name": "alice"
})).unwrap();
transport.expect("/users/get", canned);

let exit = call::<GetUser, User, _>(GET_USER, GetUser { id: 42 })
    .run_with(transport)
    .await;

let user = exit.ok().unwrap();
assert_eq!(user, User { id: 42, name: "alice".into() });
# }
```

## Custom transport

Implement [`RpcTransport`] for any byte pipe — WebSocket, gRPC, IPC,
in-memory:

```rust,ignore
pub trait RpcTransport: Send + Sync + 'static {
    fn send(
        &self,
        path: &'static str,
        method: Method,
        request_body: Vec<u8>,
    ) -> AsyncResult<Result<Vec<u8>, RpcError>>;
}
```

The trait is intentionally non-generic (no `Schema` bounds) so it
stays object-safe and trivial to mock. `call::<Req, Resp, _>`
generic-ifies on top.

## What's coming

- **Server side** — `Server::handle(endpoint, |req| Effect<Resp, E, R>)`
  with axum / hyper routing.
- **`HttpApi`** — a single schema-first declaration that generates BOTH
  the typed client and the server router.
- **Streaming endpoints** — `EndpointStream<Req, Resp>` returning
  `Stream<Resp, RpcError, R>`.
- **Middleware** — auth, retries, timeouts as transport wrappers.

[`SchemaError`]: ../data/schemas.md#structured-errors
[`Schema`]: ../data/schemas.md
