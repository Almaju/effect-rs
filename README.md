# effect-rs

A functional effect framework for Rust, porting the design and
ergonomics of [Effect-TS] to the Rust ecosystem.

`effect-rs` brings typed errors, typed dependencies, structured
concurrency, schemas, layers, and observability to async Rust —
without sacrificing the ownership and zero-cost abstractions that make
Rust Rust.

> **Status: pre-alpha.** API is stabilising but not yet locked. No
> crates.io release yet; depend by git for now. See the [roadmap].

## A first taste

```rust
use effect::{Effect, Schema, Newtype};

#[derive(Debug, Clone, Newtype, Schema)]
pub struct UserId(u64);

#[derive(Debug, Schema)]
pub struct User {
    pub id: UserId,
    pub name: String,
    pub admin: bool,
}

#[tokio::main]
async fn main() {
    let program = Effect::<_, String, ()>::block(|g| async move {
        let a = g.run(Effect::<_, String, ()>::succeed(20)).await?;
        let b = g.run(Effect::<_, String, ()>::succeed(22)).await?;
        Ok::<i32, String>(a + b)
    });

    let answer = program.execute().await;
    assert_eq!(answer.ok(), Some(42));

    // JSON in / out / spec from a single declaration:
    let json = serde_json::json!({ "id": 7, "name": "alice", "admin": true });
    let parsed: User = User::parse_json(&json).unwrap();
    let _spec = User::json_schema();
}
```

## The 21-crate workspace

| Crate                | What                                                                |
| -------------------- | ------------------------------------------------------------------- |
| **`effect`**         | core runtime — `Effect<A, E, R>`, `Cause`/`Exit`, fibers, scope, retry/repeat, sync primitives |
| `effect-macros`      | derive macros: `Newtype`, `Brand`, `Schema`                         |
| `effect-schema`      | Schema trait + primitives + containers + JSON Schema export         |
| `effect-data`        | persistent `Chunk` / `HashMap` / `HashSet` (via `im`)               |
| `effect-stream`      | `Stream<A, E, R>` over `futures::Stream`                            |
| `effect-config`      | env / map loaders parsing through `Schema`                          |
| `effect-platform`    | `Clock` / `Random` / `FileSystem` services + Live impls             |
| `effect-logging`     | thin wrappers over `tracing`                                        |
| `effect-printer`     | Wadler doc combinators + ANSI                                       |
| `effect-cli`         | argv parser + Printer-rendered help                                 |
| `effect-http`        | `HttpClient` over `reqwest` + `FakeHttpClient`                      |
| `effect-http-server` | `Router` + `Handler` over `hyper` 1.x                               |
| `effect-rpc`         | typed `Endpoint<Req, Resp>` + Schema encode/decode                  |
| `effect-sql`         | abstract `SqlExecutor` + `FakeSqlExecutor`                          |
| `effect-sql-sqlite`  | `sqlx::SqlitePool` driver                                           |
| `effect-sql-postgres`| `sqlx::PgPool` driver                                               |
| `effect-ai`          | `LlmProvider` trait + `FakeLlmProvider`                             |
| `effect-ai-openai`   | OpenAI `/v1/chat/completions` provider                              |
| `effect-ai-anthropic`| Anthropic `/v1/messages` provider                                   |
| `effect-workflow`    | event-sourced step journal — Temporal-lite                          |
| `effect-test`        | assertion combinators + tracing log capture                         |

## Examples

Two examples in `examples/`:

- [`todo`](./examples/todo/) — the original showcase: trait-based DI,
  three composition styles (combinator / do-notation / from_fn),
  error handling, parallel composition, `#[derive(Schema)]` JSON I/O.
- [`notes`](./examples/notes/) — a multi-service REST-ish app
  exercising CLI entry, env-driven config, sqlite storage, hyper HTTP
  server, schema-validated requests/responses, structured logging.
  End-to-end test against a real listener.

```bash
cargo run -p effect-example-todo
NOTES_PORT=3000 cargo run -p effect-example-notes
```

## Design

`effect-rs` is the *philosophy* of Effect, not a transliteration.
Where Rust has the better answer, we use it.

| Effect-TS does                              | effect-rs does                              |
| ------------------------------------------- | ------------------------------------------- |
| `Brand` hack `string & Brand<"Email">`      | `#[derive(Newtype)] struct Email(String);`  |
| `Option<A>` / `Either<E, A>` library types  | std `Option` / `Result` (with extensions)   |
| HKT emulation via `* extends HKT`           | concrete trait families per type            |
| `Effect.gen(function*() { … })`             | `Effect::block(\|g\| async move { … })`     |
| `pipe(x, f, g)`                             | method chains; `pipe!(…)` macro planned     |

Full design write-up: [PLAN.md](./PLAN.md), and the
[design-decisions chapter](./docs/src/project/design-decisions.md).

## Documentation

The user manual is an mdBook in `docs/`. Build it locally:

```bash
cargo install mdbook
mdbook serve docs        # live-reloads on changes
```

Chapter map: intro, the Effect type, errors & cause, layers,
state & sync, retries, scopes, concurrency, streams, configuration,
logging, platform, pretty printing, CLI, HTTP client + server, SQL,
RPC, AI, workflows, plus a Data section (schemas, newtypes,
typeclasses, collections) and the project's roadmap + design
decisions.

## Tests

```bash
cargo test
```

Currently **~340 tests** across the workspace. Every crate has unit
tests; the `notes` example has an end-to-end integration test against
a real HTTP listener.

## Inspirations

- [Effect-TS](https://effect.website) — the direct inspiration
- [ZIO](https://zio.dev) — Effect-TS's own inspiration
- Haskell's `mtl` / `freer-simple`, PureScript's `Run`, `fp-ts`

## License

Dual-licensed under MIT or Apache-2.0, at your option.

[Effect-TS]: https://effect.website
[roadmap]: ./docs/src/project/roadmap.md
