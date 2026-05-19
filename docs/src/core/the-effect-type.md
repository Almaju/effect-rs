# The Effect Type

```rust,ignore
pub struct Effect<A, E, R> { /* … */ }
```

An `Effect<A, E, R>` is a *description* of an asynchronous, potentially
failing computation. Creating one does not start any work. Running it
produces an [`Exit<A, E>`](./errors-and-cause.md) — either a success or a
structured [`Cause<E>`](./errors-and-cause.md) (typed failure, defect, or
interruption).

| Parameter | Meaning                                              |
| --------- | ---------------------------------------------------- |
| `A`       | The successful result type.                          |
| `E`       | The typed failure channel (wrapped in `Cause::Fail`).|
| `R`       | The environment (services) the effect needs to run.  |

## Construction

| Function                                                  | Use                                                                                         |
| --------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `Effect::succeed(value)`                                  | Immediately yields `value`.                                                                 |
| `Effect::fail(error)`                                     | Immediately fails with a typed `E` (becomes `Cause::Fail`).                                 |
| `Effect::die(defect)`                                     | Immediately fails with a defect (`Cause::Die`).                                             |
| `Effect::die_message("...")`                              | Shortcut for `Effect::die(Defect::new("..."))`.                                             |
| `Effect::from_cause(cause)`                               | Fails with a pre-built `Cause`.                                                             |
| `Effect::sync(\|\| Result<A, E>)`                         | Lifts a synchronous, fallible function. **Panics inside `f` are caught into `Cause::Die`.** |
| `Effect::from_fn(\|ctx\| async move { Result<A, E> })`    | Most general; receives the environment, returns a future producing a typed `Result`.        |
| `Effect::from_fn_exit(\|ctx\| async move { Exit<A, E> })` | Like `from_fn`, but the closure produces an `Exit` directly — for emitting defects.         |
| `Effect::from(result)` (`impl From<Result<A, E>>`)        | Lifts an already-computed `Result`.                                                         |

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

| Method                                                  | Use                                                                                                 |
| ------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `.catch_all(\|e\| Effect<A, E2, R>)`                    | Recover from typed failures (`Cause::Fail`). Defects/interruption pass through.                     |
| `.catch_all_cause(\|c\| Effect<A, E2, R>)`              | Recover from *any* cause — typed failures, defects, and interruption.                               |
| `.or_else(fallback)`                                    | On typed failure, run the fallback instead (same `A`/`E`).                                          |
| `.sandbox()`                                            | Lift the full `Cause<E>` into the typed error channel; result is `Effect<A, Cause<E>, R>`.          |
| `.unsandbox()`                                          | Inverse — flattens `Effect<A, Cause<E>, R>` back to `Effect<A, E, R>`.                              |

```rust,no_run
use effect::Effect;
let safe: Effect<i32, String, ()> = Effect::<i32, String, ()>::fail("nope".to_string())
    .catch_all(|_| Effect::<i32, String, ()>::succeed(0));
```

See [Errors and Cause](./errors-and-cause.md) for the full mental model
and worked examples.

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

All four methods return [`Exit<A, E>`](./errors-and-cause.md). Convert
with `.ok()`, `.err()`, `.into_result()`, or `.into_typed_result()`.

| Method                          | When `R = …`        | What it does                              |
| ------------------------------- | ------------------- | ----------------------------------------- |
| `.execute()`                    | `R = ()`            | Run; produces `Exit<A, E>`.               |
| `.run_with(ctx)`                | any                 | Run, taking an owned `ctx`.               |
| `.run(Arc<R>)`                  | any                 | Run, sharing an `Arc<R>`.                 |
| `Runtime::new(ctx).run(&eff)`   | any                 | Long-lived runtime; reuse across calls.   |

```rust,no_run
use effect::{Effect, Exit};

# #[tokio::main] async fn main() {
let exit: Exit<i32, String> = Effect::<_, String, ()>::succeed(42).execute().await;
assert_eq!(exit.ok(), Some(42));
# }
```

See [Layers and the Runtime](./layers-and-runtime.md) for the recommended
multi-service setup, and [Errors and Cause](./errors-and-cause.md) for the
full failure model.

## What's coming

`Effect`'s public API is settling, but the internals will change. Phase 1
replaces the current closure-based representation with a stack-safe
interpreter, unlocking:

- `Effect::scoped` for resource safety (acquire/release with async release).
- `Schedule` for retries and repetition.
- `Effect::fork` + structured fiber supervision.
- `Effect::race` and friends.
- Panic catching inside `Effect::from_fn` (sync already does it).
- Proper handling of typed failures inside compound causes — see the
  *Known limitation* note under `catch_all` in the rustdoc.

The public methods above are intended to remain source-compatible.
