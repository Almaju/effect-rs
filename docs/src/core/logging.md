# Logging

`effect-logging` is a thin wrapper around [`tracing`] — Rust's de
facto structured-logging façade. Log lines are effects you compose
into a pipeline; spans wrap inner effects so the subscriber sees
context automatically.

```toml
[dependencies]
effect         = { version = "0.0.1" }
effect-logging = { version = "0.0.1" }
tracing-subscriber = "0.3"  # any subscriber will do
```

## Emit a log line

```rust,no_run
use effect_logging::{info, warn};
use effect::Effect;

# #[tokio::main] async fn main() {
let program: Effect<i32, String, ()> = Effect::block(|g| async move {
    g.run(info::<String, ()>("starting work")).await?;
    let result = 42;
    g.run(info::<String, ()>(format!("done, got {result}"))).await?;
    Ok(result)
});
# }
```

Five levels, each producing an `Effect<(), E, R>`:

| Function    | Level   |
| ----------- | ------- |
| `trace`     | TRACE   |
| `debug`     | DEBUG   |
| `info`      | INFO    |
| `warn`      | WARN    |
| `error`     | ERROR   |

The effect is generic over `E` and `R` so it fits any surrounding
chain without conversion (turbofish only needed when called standalone).

## Wrap in a span — `instrument` / `with_span`

A `tracing::Span` carries name + fields visible to the subscriber for
the whole duration of the wrapped work. `instrument` attaches a span
to any effect:

```rust,no_run
use effect_logging::instrument;
# use effect::Effect;
# let work = Effect::<i32, String, ()>::succeed(42);
let span = tracing::info_span!("fetch_user", user_id = 7);
let traced = instrument(work, span);
```

`with_span(&'static str, eff)` is the common-case shorthand:

```rust,no_run
# use effect_logging::with_span;
# use effect::Effect;
# let work = Effect::<i32, String, ()>::succeed(42);
let traced = with_span("compute", work);
```

For dynamic fields, build the span yourself with `tracing::info_span!`
and pass it to `instrument`.

## A typical wiring

```rust,no_run
use tracing_subscriber::{EnvFilter, fmt};

# fn main() {
fmt()
    .with_env_filter(EnvFilter::from_default_env())
    .with_target(false)
    .init();

// … now run your Effects; events go to stdout.
# }
```

`tracing_subscriber::fmt()` is the standard ANSI-coloured formatter.
Swap in `tracing_subscriber::registry()` + your own layers for JSON
output, OTel export, file rotation, etc.

## Why `tracing` (and not a custom logger)?

Async Rust converged on `tracing` for two reasons:

1. **Spans propagate across `.await` points.** A request-handler's
   span stays attached when work hops between fibers, even when
   `tokio::spawn`'d, via `tracing::Instrument`.
2. **One ecosystem.** Subscribers exist for plain text, JSON, OTel,
   Jaeger, Datadog, Honeycomb, and just about anything else. Picking
   another logger means re-deriving these.

`effect-logging` is a deliberately thin layer — it doesn't try to
replace `tracing`, it lets you compose it inside `Effect` pipelines
without giving up the ecosystem.

## What's coming

- **`effect-metric`** — counter / gauge / histogram effects built on
  [`metrics`](https://docs.rs/metrics).
- **`effect-otel`** — opinionated OTel subscriber setup with
  Effect-typed lifecycle (initialize / flush / shutdown).
- **Fiber-aware spans** — `Fiber::fork` will automatically clone the
  surrounding span into the child.
- **`Effect::log_failure(level, prefix)`** — automatic logging of the
  full `Cause` on failure.

[`tracing`]: https://docs.rs/tracing
