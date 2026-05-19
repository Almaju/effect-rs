# Why effect-rs

Rust gives you a great deal for free: `Result<T, E>` for typed errors,
algebraic data types for modelling domains, ownership for safe sharing,
and `async`/`await` for non-blocking I/O. So why add another framework?

Because once you build something non-trivial — a service with logging,
metrics, a database, a configuration source, retries, structured
cancellation, schemas at the boundary — Rust starts to feel a bit
*scattered*. You stitch together `tokio`, `tracing`, `serde`, `clap`,
`sqlx`, `anyhow`, `thiserror`, `config`, and so on. Each is excellent in
isolation; the seams between them are where complexity grows.

Effect-TS solved the same problem in TypeScript by collapsing all of these
concerns into a single abstraction:

> **An `Effect<A, E, R>` is a description of a computation that, when
> run, produces an `A`, may fail with an `E`, and requires services `R`.**

That single sentence buys you:

- **Typed errors** — `E` is part of the type, like `Result`, but composable
  across async, parallelism, and resources.
- **Typed dependencies** — `R` is checked at compile time. You can't run
  an effect that needs a database without providing one.
- **Structured concurrency** — `fork`, `race`, `join` come with
  supervised cancellation built in.
- **Resource safety** — `Scope` ensures `release` runs no matter how the
  computation ends.
- **Pluggable everything** — tracing, metrics, logging, retries, and
  schedules are values you compose, not mixins you bolt on.

## What's different in Rust

Rust has features TypeScript would kill for: real `enum`s, ownership,
no `null`, no implicit `any`. So effect-rs is **not** a transliteration.
Where Rust has the better answer, we use it:

| Effect-TS does                                | effect-rs does                              |
| --------------------------------------------- | ------------------------------------------- |
| `Brand` hack `string & { __brand: "Email" }`  | `#[derive(Newtype)] struct Email(String);`  |
| `Option<A>` / `Either<E, A>` library types    | std `Option` / `Result` (with extensions)   |
| HKT emulation via `* extends HKT`             | concrete trait families per type            |
| `Effect.gen(function*() { … })`               | `async { eff!(…).await? }` (`eff!` macro)   |
| `pipe(x, f, g)`                               | method chains; `pipe!(…)` macro for mixed   |

## When effect-rs is the right tool

- You're building a service with multiple cross-cutting concerns
  (logging, tracing, retries, config, multiple data sources) and the
  glue is starting to dominate.
- You want compile-time confidence that "this code path is reachable
  only if the database service has been provided".
- You want testability — swap any layer for a fake by changing one
  line at the top of your test.
- You already use Effect-TS elsewhere and want the same shape in Rust.

## When it's not

- You're writing a small CLI or library with one external dependency —
  stick with `Result`, `anyhow`, and a hand-rolled `App` struct.
- You need to interop with a large existing tokio codebase and don't
  want to introduce a new abstraction. `effect-rs` *uses* tokio, but
  it asks you to express your top-level program as `Effect`s.
