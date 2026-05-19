# Errors and Cause

Failures in `effect-rs` are richer than `Result<T, E>`. Three kinds:

| Cause variant | When it happens                                       | Recoverable with                |
| ------------- | ----------------------------------------------------- | ------------------------------- |
| `Fail(E)`     | The effect explicitly returned a typed error.         | `catch_all`, `or_else`          |
| `Die(Defect)` | A panic, broken invariant, or other bug.              | `catch_all_cause`, `sandbox`    |
| `Interrupt`   | Cooperative cancellation from another fiber (Phase 1b). | `catch_all_cause`, `sandbox`  |

Plus two compounds:

| `Sequential(a, b)` | Cause `a` was followed by cause `b` (e.g. a finalizer error after a primary failure). |
| `Parallel(a, b)`   | Cause `a` happened alongside `b` (e.g. both arms of `zip` failed concurrently).         |

The structured failure is wrapped in [`Cause<E>`]. Running an effect
produces an [`Exit<A, E>`]: either `Success(A)` or `Failure(Cause<E>)`.

## Failures vs defects

```rust,no_run
use effect::{Effect, Cause, Exit};

# #[tokio::main] async fn main() {
// A typed failure — recoverable in business logic.
let known: Effect<i32, String, ()> = Effect::fail("not found".to_string());

// A defect — a bug. NOT a typed failure.
let bug: Effect<i32, String, ()> = Effect::die_message("unexpected null");

// And panics get caught into `Die` for free:
let panicky: Effect<i32, String, ()> = Effect::sync(|| panic!("oh no"));

assert_eq!(known.execute().await.err(), Some("not found".to_string()));

let bug_exit: Exit<i32, String> = bug.execute().await;
assert!(matches!(bug_exit, Exit::Failure(Cause::Die(_))));

let panic_exit: Exit<i32, String> = panicky.execute().await;
assert!(matches!(panic_exit, Exit::Failure(Cause::Die(_))));
# }
```

The runtime **catches panics** in `Effect::sync` and converts them into
`Cause::Die`. A panic doesn't kill your program — it lands in the error
channel where you can decide what to do.

> Note: panics inside `Effect::from_fn` (async closures) currently
> propagate as a normal panic. That changes when the Phase 1b interpreter
> lands.

## `catch_all` — typed failures only

```rust,no_run
use effect::Effect;

# #[tokio::main] async fn main() {
let recovered = Effect::<i32, String, ()>::fail("oops".into())
    .catch_all(|_e| Effect::<i32, String, ()>::succeed(0));

assert_eq!(recovered.execute().await.ok(), Some(0));

// catch_all does NOT touch defects:
let still_dies = Effect::<i32, String, ()>::sync(|| panic!("bug"))
    .catch_all(|_| Effect::<i32, String, ()>::succeed(0));

assert!(still_dies.execute().await.cause().unwrap().is_die());
# }
```

This is the same separation Effect-TS draws between `catchAll` and
`catchAllCause`: business code shouldn't pretend a bug is a known error.

## `catch_all_cause` — see everything

When you really do need to inspect a defect (logging, metrics, last-resort
fallbacks), use `catch_all_cause`:

```rust,no_run
use effect::Effect;

# #[tokio::main] async fn main() {
let last_resort = Effect::<i32, String, ()>::sync(|| panic!("hardware fault"))
    .catch_all_cause(|c| {
        eprintln!("[ALERT] {c}");        // structured cause Display
        Effect::<i32, String, ()>::succeed(-1)
    });

assert_eq!(last_resort.execute().await.ok(), Some(-1));
# }
```

The closure receives the full `Cause<E>` and decides what to do — including
returning a brand-new typed error.

## `sandbox` / `unsandbox` — promote the cause into the error channel

Sometimes you want to handle defects *with the same machinery* you use for
typed errors. `sandbox()` lifts the entire `Cause<E>` into the `E` channel:

```rust,no_run
use effect::{Effect, Cause};

# #[tokio::main] async fn main() {
let sandboxed: Effect<i32, Cause<String>, ()> =
    Effect::<i32, String, ()>::sync(|| panic!("bug"))
        .sandbox();

// Now catch_all sees Die alongside Fail:
let recovered: Effect<i32, Cause<String>, ()> = sandboxed.catch_all(|c| {
    if c.is_die() {
        Effect::<i32, Cause<String>, ()>::succeed(-1)
    } else {
        Effect::from_cause(c)
    }
});
# }
```

`unsandbox()` is the inverse — it flattens an `Effect<A, Cause<E>, R>` back
to `Effect<A, E, R>`, restoring the original cause structure.

## `or_else` — fallback effect

A common shortcut for "if this fails, run that instead":

```rust,no_run
use effect::Effect;

# #[tokio::main] async fn main() {
let primary  = Effect::<i32, String, ()>::fail("nope".into());
let fallback = Effect::<i32, String, ()>::succeed(0);

assert_eq!(primary.or_else(fallback).execute().await.ok(), Some(0));
# }
```

Like `catch_all`, `or_else` only triggers on `Cause::Fail`. Defects pass
through.

## `Exit` API quick reference

```rust,no_run
use effect::Exit;

# #[tokio::main] async fn main() {
# let exit: Exit<i32, String> = Exit::Success(42);
exit.is_success();                  // bool
exit.is_failure();                  // bool
exit.cause();                       // Option<&Cause<E>>

// Consume:
exit.ok();                          // Option<A>
# let exit: Exit<i32, String> = Exit::Success(42);
exit.err();                         // Option<E>  (Fail only)
# let exit: Exit<i32, String> = Exit::Success(42);
exit.into_result();                 // Result<A, Cause<E>>
# let exit: Exit<i32, String> = Exit::Success(42);
exit.into_typed_result();           // Result<A, E>  — panics on Die/Interrupt
# }
```

`into_typed_result()` is the convenience for `async` blocks that use `?`:

```rust,no_run
use effect::Effect;
use std::sync::Arc;

# #[tokio::main] async fn main() {
let program: Effect<i32, String, ()> = Effect::from_fn(|ctx: Arc<()>| async move {
    let a = Effect::<i32, String, ()>::succeed(10)
        .run(ctx.clone()).await.into_typed_result()?;
    let b = Effect::<i32, String, ()>::succeed(32)
        .run(ctx).await.into_typed_result()?;
    Ok(a + b)
});

assert_eq!(program.execute().await.ok(), Some(42));
# }
```

The boilerplate goes away once the `eff!` macro lands (Phase 1b).

## What's coming next

- **`Cause::Interrupt`** becomes meaningful when the fiber-aware
  interpreter lands. Today it's a placeholder; you can construct it but
  nothing emits it.
- **Panic catching in async** — `Effect::from_fn` panics currently
  propagate; the new interpreter will run them on a tokio task and convert
  to `Die`.
- **Cause normalization** — duplicate causes, empty causes, and pretty
  printing of compound trees will improve once the interpreter is in.
- **`catch_all` and compound causes** — today, if you `zip` two failing
  effects and call `catch_all`, the compound `Parallel` cause doesn't
  match the `Fail` arm and conversion will panic. Use `catch_all_cause`
  or `sandbox` for these cases. Fixed properly in Phase 1b.
