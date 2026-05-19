# Errors and Cause

> **Status: Phase 1.** This chapter describes the *intended* model. Today
> `Effect<A, E, R>` produces a plain `Result<A, E>` on completion.

In Effect-TS, failures fall into three categories:

| Category       | Meaning                                                            |
| -------------- | ------------------------------------------------------------------ |
| `Fail(E)`      | An *expected* error of type `E`. The kind you handle.              |
| `Die(defect)`  | An *unexpected* failure (panic, broken invariant). Never typed.    |
| `Interrupt`    | A cooperative cancellation from another fiber.                     |

`effect-rs` will mirror this with two types:

```rust,ignore
pub enum Cause<E> {
    Fail(E),
    Die(Box<dyn std::any::Any + Send + Sync>),  // or a richer Defect type
    Interrupt(FiberId),
    Sequential(Box<Cause<E>>, Box<Cause<E>>),
    Parallel(Box<Cause<E>>, Box<Cause<E>>),
}

pub enum Exit<A, E> {
    Success(A),
    Failure(Cause<E>),
}
```

## Why distinguish them?

- **Fail** is recoverable in your business logic. `catch_all` operates here.
- **Die** is a bug. You want it logged with a full backtrace and surfaced
  to your observability stack, not silently mapped to a string.
- **Interrupt** is structural — when a parent fiber dies, its children
  receive `Interrupt`, and *they should not catch it*.

Lumping all three into `Result<A, E>` (as the current prototype does)
loses these distinctions. The Phase 1 interpreter introduces `Exit` and
`Cause` and threads them through every combinator.

## Migration story

The public API stays the same wherever possible:

- `.catch_all` continues to handle `Fail(E)`, ignoring `Die` / `Interrupt`.
- New `.catch_all_cause(|c: Cause<E>| …)` exposes the full cause when you
  *do* need it.
- New `.sandbox()` lifts `Cause<E>` into the `E` channel so you can
  inspect it like any other error.
- Panics inside `Effect::sync` and `Effect::from_fn` are caught and
  converted into `Die`, not propagated.

When Phase 1 lands this chapter is rewritten with runnable examples and
moves out of placeholder status.
