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
| 5     | **CLI + Printer (done)**           | `effect-printer`: Wadler doc combinators + ANSI styling (Bold/Italic/Underline/Dim + Color); `effect-cli`: Command/Arg/Opt/Flag spec + argv parser + Printer-rendered help. **Deferred:** subcommands, Schema-typed args, combined short flags, `--key=value` syntax. |
| 3     | Streams, STM, Config             | `Stream`/`Sink`/`Channel`; `Tx*`; declarative config.   |
| 4     | Observability + Platform         | Logger on `tracing`; Metrics; OTel; FS/Terminal/Stdio. |
| 5     | CLI + Printer                    | `effect-printer`; `effect-cli`.                        |
| 6     | HTTP, RPC, SQL                   | `effect-http` (incl. HttpApi); `effect-rpc`; sqlx-backed `effect-sql`. |
| 7     | Workflow, Cluster, AI            | Durable workflows; entity sharding; LLM abstractions.  |
| 8     | Polish                           | Benchmarks; CI hardening; 0.1 release.                 |

## Rule

A feature isn't done until its chapter in this book is written.
