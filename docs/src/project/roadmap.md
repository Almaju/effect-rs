# Roadmap

`effect-rs` is pre-alpha. The full plan — including phases, the workspace
layout, the module-by-module mapping from Effect-TS, and open architectural
questions — lives in [`PLAN.md`](https://github.com/Almaju/effect-rs/blob/main/PLAN.md)
at the repo root.

A short summary of the phases:

| Phase | Theme                            | Headline deliverable                                   |
| ----- | -------------------------------- | ------------------------------------------------------ |
| 0     | Prototype (done)                 | `Effect<A,E,R>`, trait-based Layer, todo example.      |
| 1     | **Foundations (done)**     | `Cause`/`Exit`; `Schedule`/retry/repeat; `Ref`/`Deferred`/`Queue`/`Semaphore`; `Effect::block` do-notation; `effect-macros`; interrupt/interruptible/uninterruptible via task-local fiber state; `Scope` + `acquire_release`; `Fiber` + `fork` + `race`; `for_each_par` + `on_interrupt` + `fork_scoped`. Deferred to a later pass: `effect-test` harness, full free-monad interpreter (stack safety + compound-cause handling). |
| 2     | **Data + Schema (done)**          | `Chunk`/`HashMap`/`HashSet`; typeclass families; `#[derive(Schema)]` + `#[derive(Newtype)]` + `#[derive(Brand)]`; primitives + container Schema impls; Schema → JSON Schema export. |
| 3     | **Streams, Config (done; STM deferred)** | `effect-stream`: `Stream<A,E,R>` + lazy combinators + Effect-typed terminals; `effect-config`: Schema-driven env / map loaders. **Deferred:** real STM (TxRef, TxQueue) — requires optimistic concurrency and retry that's substantial work; mock-via-Mutex would mislead. |
| 4     | **Observability + Platform (done; metric/OTel deferred)** | `effect-logging`: tracing wrappers + instrument/with_span; `effect-platform`: Clock, Random (fastrand), FileSystem (tokio::fs) with Live* impls. **Deferred:** `effect-metric`, `effect-otel`, Terminal, Stdio, Process/Command — straightforward extensions to add when needed. |
| 5     | **CLI + Printer (done)**           | `effect-printer`: Wadler doc combinators + ANSI styling; `effect-cli`: Command/Arg/Opt/Flag spec + argv parser + Printer-rendered help + `--key=value` + combined short flags. **Deferred:** subcommands, Schema-typed args. |
| 6     | **HTTP + SQL + RPC (done)**        | `effect-http`: HttpClient trait + LiveHttpClient (reqwest+rustls) + FakeHttpClient; `effect-http-server`: hyper-1.x Router + Handler + serve; `effect-sql`: abstract SqlExecutor + SqlValue + FakeSqlExecutor; `effect-sql-sqlite`: sqlx::SqlitePool driver; `effect-sql-postgres`: sqlx::PgPool driver; `effect-rpc`: typed Endpoint + Schema encode/decode + LiveHttpRpcTransport + FakeRpcTransport. **Deferred:** HttpApi (schema-first endpoint declaration), streaming endpoints, path-pattern routing, transactions, migrations. |
| 7     | **AI + Workflow (done; cluster deferred)** | `effect-ai`: `LlmProvider` trait + `ChatMessage`/`ChatRequest`/`ChatResponse`/`TokenUsage` + `FakeLlmProvider` + helpers (`complete`/`chat`/`simple`); `effect-ai-openai`: `OpenAi<H: HttpClient>` provider for OpenAI's `/chat/completions` API with 401/429/5xx mapping; `effect-workflow`: event-sourced step journal. **Deferred:** `effect-cluster`, more provider adapters (`effect-ai-anthropic`, `effect-ai-ollama`), streaming completions, workflow timers/signals. |
| 8     | Polish                           | Benchmarks; CI hardening; 0.1 release.                 |

## Rule

A feature isn't done until its chapter in this book is written.
