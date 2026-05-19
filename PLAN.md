# effect-rs — Implementation Plan

A functional-effect framework for Rust, porting the design and ergonomics of
[Effect-TS](https://github.com/Effect-TS/effect-smol) to the Rust ecosystem.

The goal is not a line-by-line port. Effect-TS exists in part to work around
TypeScript's weaknesses (brand hacks for newtypes, structural typing limits,
no real ADTs). Rust already solves several of these natively. effect-rs
inherits the *design philosophy* of Effect — typed errors, typed
dependencies, structured concurrency, schemas, layers — and re-expresses it
idiomatically in Rust.

---

## 1. Vision

Provide one batteries-included framework for writing testable, effectful,
async Rust programs. Concretely:

- **`Effect<A, E, R>`** as the unit of computation: typed success / failure /
  required environment.
- **Layers** for compile-time-checked dependency injection.
- **Fibers** for structured concurrency, supervised cancellation, scopes.
- **Schema** for parse/validate/encode with a single declaration, covering
  JSON / args / env / SQL rows.
- **Streams / Channels** for lazy, backpressured pipelines.
- **Observability** wired in: tracing, metrics, logs share the same context.
- **Platform modules**: filesystem, HTTP, terminal, command, all expressed
  as effects so they're injectable and testable.
- **Ecosystem bridges**: CLI (clap-style ergonomics), SQL (sqlx-backed), RPC,
  OpenTelemetry, workflow, cluster.

### Non-goals

- A faithful 1:1 port of every TypeScript file. Where Rust has a better
  native answer (enums, traits, `?`, `Result`, ownership), we use it.
- Reimplementing async runtime primitives. We build on `tokio` (and stay
  runtime-agnostic where feasible).
- HKT emulation acrobatics. Rust lacks higher-kinded types; we expose
  per-type modules rather than generic typeclass hierarchies.

---

## 2. Current state

The repo already contains a credible Effect prototype:

- `src/lib.rs` — `Effect<A, E, R>` with `succeed`, `fail`, `sync`, `from_fn`,
  `map`, `map_error`, `flat_map`, `tap`, `catch_all`, `or_else`, `zip`,
  `zip_with`, `provide`, `ask`, `Runtime`, `From<Result>`. 22 passing tests.
- `src/todo/` — worked example showing trait-based DI (`HasRepo`,
  `HasLogger`), layered context (`Layer::new().with_repo(..).with_logger()`),
  business logic generic over `R: HasRepo + HasLogger`, compile-time
  enforcement of required services.
- `src/main.rs` — demo of combinator style, do-notation style, error
  handling, parallel composition, and `.provide()`.

This is a solid Phase-0 foundation. The key gap vs Effect-TS: there's no
**fiber/scheduler** layer — `flat_map` is built directly on async. Real
Effect has a programmable runtime (interruption, supervision, scopes,
fairness), and we'll need one to support Streams, Scope, Fiber, Schedule
properly.

---

## 3. Architectural decisions (the hard part)

These are the choices that shape everything else. Each lists the tradeoff
so we can revisit.

### 3.1 Fiber model
Build a lightweight interpreter on top of `tokio`: an `Effect` becomes a
free-monad-style program (`Succeed`, `Fail`, `Sync`, `Async`, `FlatMap`,
`Fold`, `Fork`, `WithScope`, ...) that the runtime steps through, tracking
fiber identity, interruption state, and the current scope. This unlocks:
- Cooperative cancellation (`interrupt`)
- Structured concurrency via `Scope`
- Supervision (a fiber's children die with it)
- Stack-safety regardless of nesting depth

**Tradeoff:** more upfront work than the current `Arc<dyn Fn>` closure form,
but the closure form can't express interruption or scopes correctly. We
keep the current API surface; the change is internal.

### 3.2 Higher-kinded types
Rust has no HKT. effect-ts's `Effect.gen` / typeclasses don't translate.
Approach:
- Don't try to emulate `Functor`/`Monad` generically. Provide methods
  per-type (`Effect::map`, `Option::map`, `Stream::map`).
- Use **GATs** where they suffice (e.g. `Pipeable`).
- Provide a `pipe!` macro for the `|>` pattern.
- For `gen`-style code, lean on async blocks with a thin `eff!` macro that
  desugars `let x = eff!(some_effect)?;` into the equivalent run.

### 3.3 Newtypes & brands
Rust has tuple structs — no hack needed. Provide a `#[derive(Newtype)]`
proc-macro that generates: `From`/`Into`, `Display`, `Deref` opt-in,
serde glue, and `Brand`-style smart constructors with refinement.

### 3.4 Schema
Effect's Schema is its crown jewel — one declaration, many uses. In Rust:
- Lean on the type system; declarations are real types, not phantom AST.
- Provide a `#[derive(Schema)]` proc-macro that materializes a
  `Schema<A>` descriptor at compile time.
- Schemas compose to: serde (`serde_json`), `clap`-style CLI args, env
  config, SQL row mapping, OpenAPI / JSON Schema output, arbitrary
  generators (`proptest`).
- Refinements (`Brand`, `Positive`, `MaxLength<100>`) via const generics
  where possible, trait objects otherwise.

### 3.5 Context / Layer
Keep the current trait-based approach (`HasRepo + HasLogger`) — it
compile-time-checks dependencies, which is exactly what Effect-TS's
`Context.Tag` system buys you. Add:
- A `tag!` macro for boilerplate-free service declaration.
- `Layer` becomes a real type with `merge`, `provide`, `memoize`,
  `scoped` operators — including failure during construction
  (`Layer<R, E>` where `E` is the build error).
- `ManagedRuntime` for long-lived app contexts.

### 3.6 Errors
Effect-TS distinguishes **expected** errors (`E` channel) from **defects**
(unexpected panics, `Cause.Die`). Mirror this:
- `E` is whatever the user picks — typically an `enum` (use `thiserror`
  where convenient).
- `Cause<E>` captures the full failure tree: `Fail(E)`, `Die(anyhow)`,
  `Interrupt(FiberId)`, `Sequential`, `Parallel`.
- `Exit<A, E>` = `Success(A) | Failure(Cause<E>)`.

### 3.7 Async ecosystem alignment
- Runtime: `tokio` by default, abstract behind a feature flag for `smol` /
  `async-std` later if demanded.
- Tracing: build on top of [`tracing`](https://docs.rs/tracing) — Effect
  spans become `tracing::Span`s.
- HTTP: `hyper` + `reqwest` under the hood.
- SQL: `sqlx`.
- CLI: own parser modeled on Effect CLI's combinator API, with `clap`
  interop for users who want it.

---

## 4. Workspace layout

Move from single-crate `lab` to a Cargo workspace mirroring effect-smol's
package structure.

```
effect-rs/
├── Cargo.toml                 # workspace
├── crates/
│   ├── effect/                # the core: Effect, Layer, Runtime, Fiber, Scope, …
│   ├── effect-macros/         # proc-macros: derive(Schema), derive(Newtype), tag!, eff!
│   ├── effect-typeclass/      # Eq, Order, Combiner, Reducer — concrete trait families
│   ├── effect-data/           # Option, Result, Chunk, HashMap/Set, Trie, Graph, …
│   ├── effect-stream/         # Stream, Sink, Channel, Pull, Take
│   ├── effect-stm/            # Tx* — software transactional memory
│   ├── effect-schema/         # Schema, AST, transformations, JSON Schema export
│   ├── effect-config/         # Config, ConfigProvider, env / file / k8s loaders
│   ├── effect-logging/        # Logger, LogLevel, structured logging on `tracing`
│   ├── effect-metric/         # Metric, counters/gauges/histograms
│   ├── effect-tracer/         # spans/links — bridges to OpenTelemetry
│   ├── effect-platform/       # FileSystem, Path, Terminal, Stdio, Process, Command
│   ├── effect-platform-tokio/ # tokio-backed implementation
│   ├── effect-printer/        # pretty printer + ANSI renderer
│   ├── effect-cli/            # Command, Args, Options, Help, REPL
│   ├── effect-otel/           # OpenTelemetry exporters
│   ├── effect-http/           # HttpApi, HttpClient, HttpServer, middleware
│   ├── effect-rpc/            # typed RPC over any transport
│   ├── effect-sql/            # Statement, Migrator, Pool — sqlx-backed
│   ├── effect-sql-postgres/   # driver-specific glue
│   ├── effect-sql-sqlite/
│   ├── effect-workflow/       # durable workflows
│   ├── effect-cluster/        # entity sharding
│   ├── effect-ai/             # provider-agnostic LLM abstractions
│   └── effect-test/           # `it_effect!`, layer-overriding test harness
├── examples/
│   ├── todo/                  # ← migrate current src/todo here
│   ├── http-server/
│   ├── cli/
│   └── workflow/
├── docs/                      # mdBook source — see §6
└── xtask/                     # local dev commands (release, docs build, lint)
```

A `effect` umbrella crate re-exports the most-used items so the simple
case is `use effect::prelude::*;`.

---

## 5. Phased roadmap

Milestones are sized so each ends with something runnable, tested, and
documented. Don't ship phase N until phase N-1's chapter in the book exists.

### Phase 1 — Foundations (the Effect core, done right)
**Outcome:** the prototype hardened, restructured into a workspace, with a
real interpreter underneath.

- [ ] Reshape repo into the workspace layout above; move existing prototype
      into `crates/effect`.
- [ ] Define the program ADT (`Op<A, E, R>`) and write the stack-safe
      interpreter (`Fiber::run`).
- [ ] Introduce `Cause<E>` and `Exit<A, E>`; thread them through the API.
- [ ] Implement `interrupt`, `uninterruptible`, `on_interrupt`.
- [ ] `Scope` + `Resource` (acquire/release), with `Effect::scoped`.
- [ ] `Schedule` (recurs, exponential, jittered) + `Effect::retry`,
      `Effect::repeat`.
- [ ] `Ref`, `Deferred`, `Queue`, `PubSub`, `Semaphore`.
- [ ] `Effect::fork`, `Effect::race`, `Effect::for_each_par`.
- [ ] `Layer` upgrade: `merge`, `provide_merge`, `scoped`, failable
      construction.
- [ ] `effect-macros`: `tag!`, `eff!`, basic `pipe!`.
- [ ] `effect-test` crate: harness that swaps layers per test.
- [ ] mdBook chapters: "Why effect-rs", "Your first effect", "Errors and
      Cause", "Scopes and resources", "Layers and the Runtime".

### Phase 2 — Data + Schema
**Outcome:** rich data types + a derive-driven schema system, with the
todo example rewritten to use them.

- [ ] `effect-data`: `Chunk`, `HashMap`, `HashSet`, persistent variants
      (use `im` crate as base, wrap with Effect ergonomics).
- [ ] `Option<A>` / `Result<A,E>` extension traits (Effect-style helpers
      that don't conflict with std).
- [ ] `effect-typeclass`: `Equivalence`, `Order`, `Combiner` (Semigroup),
      `Reducer` (Foldable-ish) as concrete trait families.
- [ ] `effect-schema` v1: `Schema<A>` trait, derive macro, primitive
      schemas, struct/enum derive, transformations, refinements.
- [ ] Schema → JSON via serde glue; Schema → JSON Schema output.
- [ ] `Brand` + `Newtype` derive.
- [ ] mdBook chapters: "Schemas", "Newtypes and brands", "Working with
      collections".

### Phase 3 — Streams, STM, Config
- [ ] `effect-stream`: `Stream<A, E, R>`, `Sink`, `Channel`, common ops
      (`map`, `filter`, `chunk`, `mapEffect`, `mapEffectPar`,
      `groupedWithin`, `throttle`, `merge`, `broadcast`).
- [ ] `effect-stm`: `TxRef`, `TxQueue`, `TxSemaphore`, `atomically`.
- [ ] `effect-config`: declarative config with env / file / argv / Vault
      providers; failures land in `Cause`.
- [ ] mdBook: "Streams", "Transactional memory", "Configuration".

### Phase 4 — Observability + Platform
- [ ] `effect-logging` on `tracing`, structured log records, level config.
- [ ] `effect-metric` with pluggable backends.
- [ ] `effect-tracer` integrated with `tracing` + OTel exporters
      (`effect-otel`).
- [ ] `effect-platform` traits: `FileSystem`, `Path`, `Terminal`, `Stdio`,
      `Process`, `Command`, `Clock`, `Random`.
- [ ] `effect-platform-tokio` concrete impls.
- [ ] mdBook: "Tracing across fibers", "Metrics", "Platform services and
      portability".

### Phase 5 — CLI + Printer
- [ ] `effect-printer`: doc combinators + ANSI renderer.
- [ ] `effect-cli`: `Command`, `Args`, `Options`, `Flags`, generated help,
      shell completion, REPL primitives.
- [ ] mdBook: "Building a CLI".

### Phase 6 — HTTP, RPC, SQL
- [ ] `effect-http`: client + server + middleware on `hyper`.
- [ ] `HttpApi`: schema-first endpoint declarations → server router +
      typed client + OpenAPI doc.
- [ ] `effect-rpc`: typed request/response over HTTP / WebSocket / TCP.
- [ ] `effect-sql` on `sqlx`; pool, migrations, schema-driven row mapping.
- [ ] Driver crates: `effect-sql-postgres`, `effect-sql-sqlite`.
- [ ] mdBook: "HTTP APIs", "Typed RPC", "SQL".

### Phase 7 — Workflow, Cluster, AI
- [ ] `effect-workflow`: durable workflows, persistence backends.
- [ ] `effect-cluster`: sharding, entity addressing.
- [ ] `effect-ai`: provider-agnostic LLM abstractions; ports of Effect's
      `ai` package patterns.
- [ ] mdBook: end-to-end app chapter combining all of the above.

### Phase 8 — Polish
- [ ] Benchmark suite; `criterion` for hot paths.
- [ ] `cargo-deny`, `cargo-msrv`, `cargo-semver-checks` in CI.
- [ ] Public 0.1 release of `effect`, `effect-macros`, `effect-data`,
      `effect-schema`. Other crates 0.0.x for now.

---

## 6. Documentation strategy

Two layers, both authoritative.

### rustdoc (per-crate)
Every public item has a doc comment with an example. We compile examples
with `cargo test --doc` so they don't rot.

### mdBook — the user manual
`docs/` contains a mdBook with a structure modeled on Effect's website:

```
docs/
├── book.toml
└── src/
    ├── SUMMARY.md
    ├── intro/
    │   ├── why-effect-rs.md
    │   ├── installation.md
    │   └── quickstart.md
    ├── core/
    │   ├── the-effect-type.md
    │   ├── errors-and-cause.md
    │   ├── scopes-and-resources.md
    │   ├── layers-and-runtime.md
    │   ├── interruption.md
    │   └── concurrency.md
    ├── data/
    │   ├── option-and-result.md
    │   ├── collections.md
    │   ├── newtypes.md
    │   └── schema.md
    ├── streams/
    ├── observability/
    ├── platform/
    ├── cli/
    ├── http-and-rpc/
    ├── sql/
    ├── workflows/
    └── recipes/
        ├── testing.md
        ├── migration-from-tokio.md
        └── migration-from-fp-ts.md
```

Conventions:
- Every chapter starts with **"What you'll learn"** and ends with
  **"What you can build now"**.
- Code blocks are tagged `rust,no_run` for non-runnable examples and
  `rust,editable` for ones extracted into `examples/`.
- A `xtask doc` command builds rustdoc + mdBook into a single static site.

We start the book in Phase 1 and grow it with each phase — the rule is:
**a feature isn't done until its chapter is written.**

---

## 7. Mapping: Effect-TS module → effect-rs home

Selective; some modules collapse into Rust standard types.

| Effect-TS                           | effect-rs                                    |
| ----------------------------------- | -------------------------------------------- |
| `Effect`                            | `effect::Effect`                             |
| `Exit`, `Cause`, `FiberId`          | `effect::{Exit, Cause, FiberId}`             |
| `Layer`, `Context`, `Tag`           | `effect::{Layer, Context}` + `tag!`          |
| `Runtime`, `ManagedRuntime`         | `effect::{Runtime, ManagedRuntime}`          |
| `Scope`, `Resource`                 | `effect::{Scope, Resource}`                  |
| `Fiber`, `FiberHandle`, `FiberSet`  | `effect::fiber::*`                           |
| `Ref`, `Deferred`, `Queue`, `PubSub`| `effect::{Ref, Deferred, Queue, PubSub}`     |
| `Stream`, `Sink`, `Channel`         | `effect_stream::*`                           |
| `Tx*` (STM)                         | `effect_stm::*`                              |
| `Schedule`                          | `effect::Schedule`                           |
| `Schema`, `SchemaAST`               | `effect_schema::*` + `#[derive(Schema)]`     |
| `Option`, `Result`                  | extension traits on std types                |
| `Chunk`, `HashMap`, `HashSet`       | `effect_data::{Chunk, HashMap, HashSet}`     |
| `Brand`, `Newtype`                  | `#[derive(Newtype, Brand)]`                  |
| `Equal`, `Equivalence`, `Order`     | `effect_typeclass::*`                        |
| `Combiner`, `Reducer`               | `effect_typeclass::{Combiner, Reducer}`      |
| `Config`, `ConfigProvider`          | `effect_config::*`                           |
| `Logger`, `LogLevel`                | `effect_logging::*` (on `tracing`)           |
| `Metric`                            | `effect_metric::*`                           |
| `Tracer`                            | `effect_tracer::*` + `effect_otel`           |
| `Console`, `Terminal`, `Stdio`      | `effect_platform::*`                         |
| `FileSystem`, `Path`, `Command`     | `effect_platform::*`                         |
| `Clock`, `Random`                   | `effect_platform::{Clock, Random}`           |
| `Printer`                           | `effect_printer::*`                          |
| `cli`                               | `effect_cli`                                 |
| `http`, `httpapi`                   | `effect_http`                                |
| `rpc`                               | `effect_rpc`                                 |
| `sql*`                              | `effect_sql*`                                |
| `workflow`                          | `effect_workflow`                            |
| `cluster`                           | `effect_cluster`                             |
| `ai`                                | `effect_ai`                                  |
| `vitest`                            | `effect_test`                                |
| `Match`, `Pipeable`, `Function`     | `pipe!`, `match!` macros + small helpers     |
| `HKT`, `Unify`                      | dropped — Rust idioms instead                |

---

## 8. Open questions

These need a decision (or a spike) before they bite us.

1. **Pipe ergonomics.** Method chaining works for `Effect` itself, but for
   cross-module composition (`pipe(x, foo, bar, baz)`), we likely want a
   `pipe!` macro. Investigate whether trait-based fluent style alone is
   enough.
2. **Generator-style sugar.** Effect's `Effect.gen` is its most-loved API.
   We can approximate it with `async { effect1.run(ctx).await?; ... }`,
   but that requires plumbing the context. Worth investigating an `eff!`
   macro that hides context threading.
3. **Schema and serde.** Should `Schema<A>` *be* a serde
   serializer/deserializer, or should it sit alongside serde with adapters?
   Recommend the latter — keep serde compatibility but don't subordinate
   ourselves to it.
4. **Async runtime neutrality.** Start tokio-only; revisit when there's
   demonstrated demand.
5. **Naming.** `effect-rs` for the project; published crates simply named
   `effect`, `effect-schema`, etc. Confirm with the user before reserving
   on crates.io.
6. **MSRV.** Pin to current stable; document a 6-month support window.
7. **License.** Effect-TS is MIT. Default to MIT-OR-Apache-2.0 (the Rust
   convention) unless preferred otherwise.

---

## 9. Immediate next steps (Phase 1, week 1)

1. Confirm this plan with the user; nail down §8 questions.
2. Convert `lab` → workspace; move existing prototype to `crates/effect`,
   move `src/todo/` to `examples/todo/`.
3. Wire mdBook scaffold + `xtask doc` command.
4. Spike the interpreter: define `Op<A, E, R>` enum, write a tiny
   stack-safe runner, port the existing tests over.
5. Open a tracking issue per phase.

If you'd like, the very next concrete action I'd take is **step 2 + 3**
(workspace + mdBook scaffold) — they're mechanical, unlock everything
else, and don't commit us to any architectural choice we'd need to
revisit later.
