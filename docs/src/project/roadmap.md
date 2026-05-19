# Roadmap

`effect-rs` is pre-alpha. The full plan — including phases, the workspace
layout, the module-by-module mapping from Effect-TS, and open architectural
questions — lives in [`PLAN.md`](https://github.com/Almaju/effect-rs/blob/main/PLAN.md)
at the repo root.

A short summary of the phases:

| Phase | Theme                            | Headline deliverable                                   |
| ----- | -------------------------------- | ------------------------------------------------------ |
| 0     | Prototype (done)                 | `Effect<A,E,R>`, trait-based Layer, todo example.      |
| 1     | Foundations (partial — see below) | `Cause`/`Exit`; `Schedule`/retry/repeat; `Ref`/`Deferred`/`Queue`/`Semaphore`; `Effect::block` do-notation; `effect-macros`. **Still pending:** interpreter rewrite, `Scope`, `Fiber`, `race`/`fork`/structured interruption, `effect-test`. |
| 2     | **Data + Schema (done)**          | `Chunk`/`HashMap`/`HashSet`; typeclass families; `#[derive(Schema)]` + `#[derive(Newtype)]` + `#[derive(Brand)]`; primitives + container Schema impls; Schema → JSON Schema export. |
| 3     | Streams, STM, Config             | `Stream`/`Sink`/`Channel`; `Tx*`; declarative config.   |
| 4     | Observability + Platform         | Logger on `tracing`; Metrics; OTel; FS/Terminal/Stdio. |
| 5     | CLI + Printer                    | `effect-printer`; `effect-cli`.                        |
| 6     | HTTP, RPC, SQL                   | `effect-http` (incl. HttpApi); `effect-rpc`; sqlx-backed `effect-sql`. |
| 7     | Workflow, Cluster, AI            | Durable workflows; entity sharding; LLM abstractions.  |
| 8     | Polish                           | Benchmarks; CI hardening; 0.1 release.                 |

## Rule

A feature isn't done until its chapter in this book is written.
