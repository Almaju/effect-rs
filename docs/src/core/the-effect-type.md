# The Effect Type

```rust,ignore
pub struct Effect<A, E, R> { /* … */ }
```

An `Effect<A, E, R>` is a *description* of an asynchronous, potentially
failing computation. Creating one does not start any work. Running it
produces a `Result<A, E>`.

| Parameter | Meaning                                              |
| --------- | ---------------------------------------------------- |
| `A`       | The successful result type.                          |
| `E`       | The typed error channel.                             |
| `R`       | The environment (services) the effect needs to run.  |

## Construction

| Function                                                                       | Use                                                          |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------ |
| `Effect::succeed(value)`                                                       | An effect that immediately yields `value`.                   |
| `Effect::fail(error)`                                                          | An effect that immediately fails with `error`.               |
| `Effect::sync(\|\| Result<A, E>)`                                              | Lift a synchronous, fallible function.                       |
| `Effect::from_fn(\|ctx\| async move { … })`                                    | Most general; receives the environment, returns a future.    |
| `Effect::from(result)` (`impl From<Result<A, E>>`)                             | Lift an already-computed `Result`.                           |

```rust,no_run
use effect::Effect;
use std::sync::Arc;

let immediate = Effect::<_, String, ()>::succeed(42);
let problem   = Effect::<i32, _, ()>::fail("boom".to_string());
let parsed    = Effect::<_, String, ()>::sync(|| "21".parse::<i32>().map_err(|e| e.to_string()));

let async_eff: Effect<i32, String, ()> = Effect::from_fn(|_: Arc<()>| async {
    Ok(tokio::time::Duration::from_millis(0).as_millis() as i32 + 42)
});
```

## Transformation

| Method                 | Shape                                              |
| ---------------------- | -------------------------------------------------- |
| `.map(f)`              | `(A) -> B`     — transform success                 |
| `.map_error(f)`        | `(E) -> E2`    — transform failure                 |
| `.flat_map(f)`         | `(A) -> Effect<B, E, R>` — sequence effects        |
| `.tap(f)`              | `(&A) -> ()`   — side-effect on success            |
| `.as_value(v)`         | replace success with a constant                    |
| `.void()`              | discard the success value                          |

```rust,no_run
use effect::Effect;
let program = Effect::<_, String, ()>::succeed(10)
    .map(|x| x + 5)                              // 15
    .flat_map(|x| Effect::succeed(x * 2))        // 30
    .map(|x| x + 12);                            // 42
```

## Error handling

| Method                                          | Use                                                         |
| ----------------------------------------------- | ----------------------------------------------------------- |
| `.catch_all(\|e\| Effect<A, E2, R>)`            | Recover from any error, possibly changing the error type.   |
| `.or_else(fallback)`                            | If this fails, run the fallback (same `A`/`E`).             |

```rust,no_run
use effect::Effect;
let safe: Effect<i32, String, ()> =
    Effect::fail("nope".to_string()).catch_all(|_| Effect::succeed(0));
```

## Concurrency

| Method                                    | Shape                                                   |
| ----------------------------------------- | ------------------------------------------------------- |
| `.zip(other)`                             | Run two effects concurrently; collect `(A, B)`.         |
| `.zip_with(other, \|a, b\| c)`            | Combine two concurrent results.                         |

```rust,no_run
use effect::Effect;
let sum = Effect::<_, String, ()>::succeed(20)
    .zip_with(Effect::succeed(22), |a, b| a + b);
```

> Fork/join/race/structured-interruption are part of Phase 1's interpreter
> work — see [Roadmap](../project/roadmap.md).

## Environment

| Method                | Use                                                                     |
| --------------------- | ----------------------------------------------------------------------- |
| `Effect::ask()`       | Yield the current environment (Reader-monad's `ask`).                   |
| `.provide(ctx)`       | Eliminate the `R` requirement by baking in a concrete environment.      |

```rust,no_run
use effect::Effect;
struct Config { value: i32 }

let needs_config = Effect::<i32, String, Config>::from_fn(|ctx| async move {
    Ok(ctx.value * 2)
});

let standalone = needs_config.provide(Config { value: 21 });   // Effect<i32, String, ()>
```

## Running

| Method                          | When `R = …`        | What it does                              |
| ------------------------------- | ------------------- | ----------------------------------------- |
| `.execute()`                    | `R = ()`            | Run; produces `Result<A, E>`.             |
| `.run_with(ctx)`                | any                 | Run, taking an owned `ctx`.               |
| `.run(Arc<R>)`                  | any                 | Run, sharing an `Arc<R>`.                 |
| `Runtime::new(ctx).run(&eff)`   | any                 | Long-lived runtime; reuse across calls.   |

See [Layers and the Runtime](./layers-and-runtime.md) for the recommended
multi-service setup.

## What's coming

The current `Effect` is built on closure composition. Phase 1 of the
roadmap replaces the internals with a stack-safe interpreter so we can
support:

- `Cause<E>` and `Exit<A, E>` to distinguish expected failures from
  defects and interruption.
- `Effect::scoped` for resource safety.
- `Schedule` for retries and repetition.
- `Effect::fork` + structured fiber supervision.
- `Effect::race` and friends.

The public API above is intended to remain source-compatible.
