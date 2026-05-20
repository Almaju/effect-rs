# Notes — a multi-service example

`examples/notes` is the closest thing to a real app in the repo. It
exercises:

- [`effect-cli`](../core/cli.md) — entry-point arg parsing
- [`effect-config`](../core/config.md) — env-driven configuration
- [`effect-logging`](../core/logging.md) — structured logging
- [`effect-schema`](../data/schemas.md) — request/response validation
- [`effect-sql`](../core/sql.md) + [`effect-sql-sqlite`](../core/sql.md#sqlite-driver--effect-sql-sqlite) — persistence
- [`effect-http-server`](../core/server.md) — REST-ish API
- [`Newtype`](../data/newtypes.md) — typed `NoteId`
- [`effect`](../core/the-effect-type.md) itself — `Effect::block`-style do-notation throughout the storage layer

```bash
cargo run -p effect-example-notes
# Or with config:
NOTES_PORT=3000 NOTES_DB_PATH=:memory: cargo run -p effect-example-notes
```

## Layout

```
examples/notes/
├── Cargo.toml
├── src/
│   ├── lib.rs       — facade for integration tests
│   ├── main.rs      — entry: tracing setup, config load, bind, serve
│   ├── config.rs    — AppConfig schema (port, db_path)
│   ├── model.rs     — NoteId newtype, Note + request/response schemas
│   ├── repo.rs      — sqlite persistence (create_note / list_notes / get_note)
│   └── api.rs       — HTTP handlers + the build_router function
└── tests/
    └── end_to_end.rs — spins up a real listener, exercises every endpoint
```

## Cross-cutting wiring

The `main.rs` boots everything:

```rust,ignore
// 1. Logging first so config errors are visible.
tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
    .init();

// 2. CLI: --help, fall back to "serve" otherwise.
let cmd = Command::new("notes", "A small REST-ish notes service")
    .arg(Arg::new("command", "Subcommand to run (only 'serve' is supported)"));
match parse(&cmd, std::env::args().collect()) { /* handle help, MissingArg, etc. */ }

// 3. Config from env (NOTES_* with defaults).
let cfg = from_env::<AppConfig>("NOTES_").execute().await
    .ok().unwrap_or_else(AppConfig::defaults);

// 4. Open the DB (creates schema if missing).
let db = open_db(&cfg.db_path).await.expect("open db");

// 5. Build the router + bind + serve.
let listener = TcpListener::bind(format!("0.0.0.0:{}", cfg.port)).await.unwrap();
serve(listener, build_router(db)).await.unwrap();
```

## The handler shape

Every handler is a `Fn(Request) -> Future<Output = Result<Response, ServerError>>`
that closes over `Arc<SqliteExecutor>` for state:

```rust,ignore
.post("/notes/create", move |req: Request| {
    let db = db_create.clone();
    async move { handle(req, db, create).await }
})
```

`handle` is a small generic helper that does the
parse → run → encode dance:

```rust,ignore
async fn handle<I, O, F>(req: Request, db: Arc<SqliteExecutor>, f: F)
    -> Result<Response, ServerError>
where
    I: Schema + Send + 'static,
    O: Schema + Send + 'static,
    F: FnOnce(I) -> Effect<O, String, SqliteExecutor> + Send + 'static,
{
    tracing::info!(target: "notes", method = ?req.method, path = %req.path, "request");

    let value = serde_json::from_slice::<Value>(&req.body)
        .map_err(/* → 400 */)?;
    let input = I::parse_json(&value)
        .map_err(/* → 400 with field path */)?;

    let executor = SqliteExecutor::with_pool(db.pool().clone());
    let exit = f(input).run_with(executor).await;

    match exit {
        Exit::Success(output) => Ok(Response::json(200, serde_json::to_vec(&output.encode_json())?)),
        Exit::Failure(cause)  => Ok(Response::internal_error(format!("{cause}"))),
    }
}
```

That's about 30 lines of glue total. Everything else is business
logic written in plain `Effect`-typed Rust.

## Storage layer

Sqlite via `effect-sql-sqlite`. The repo defines a one-connection pool
(so `:memory:` databases work — each sqlite connection otherwise gets
its own private memory DB), and a tiny helper to clone the underlying
pool into a fresh `SqliteExecutor` for handler-local use.

Ids are generated client-side from `SystemTime::now().as_nanos()` plus
a process-local counter — a deliberate simplification that side-steps
sqlx pool-state quirks around `last_insert_rowid()`/`RETURNING`. A
real app would use UUIDs or Snowflake-style ids.

## End-to-end test

`tests/end_to_end.rs` spins up a real server on `127.0.0.1:0`,
hits it with `reqwest`, and asserts on the response bodies:

```rust,ignore
async fn spawn() -> SocketAddr {
    let db = open_db(":memory:").await.expect("init in-memory db");
    let router = build_router(db);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { let _ = serve(listener, router).await; });
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

#[tokio::test]
async fn create_then_list_then_get_roundtrip() {
    let addr = spawn().await;
    let client = reqwest::Client::new();
    // POST /notes/create → 200, then list shows it, then get-by-id returns it.
}
```

Five tests cover the happy path, bad JSON, missing schema field,
unknown id, and unknown route.

## What it deliberately doesn't do

- **Authentication / sessions** — no auth middleware yet.
- **Pagination** — list returns everything.
- **Path patterns** — `/notes/get` takes the id in the JSON body
  rather than `/notes/:id` (path-pattern routing is a planned
  extension to `effect-http-server`).
- **Migrations** — `CREATE TABLE IF NOT EXISTS` on startup; a real
  app would use a migration library.

Each of these is an extension a follow-on slice could add without
restructuring what's here.
