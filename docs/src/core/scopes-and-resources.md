# Scopes and Resources

> **Status: Phase 1.** This chapter describes the *intended* model;
> `Scope` is not yet implemented.

Rust gives you `Drop` for synchronous, owned resources. That's not
enough when:

- The resource is acquired asynchronously (`async fn connect()`).
- Release itself is asynchronous (`async fn close()`).
- The resource needs to outlive a specific function call but die when
  its enclosing scope does.
- A parent fiber's cancellation must roll back every resource its
  children acquired, *concurrently*.

`Scope` is the structured-concurrency answer:

```rust,ignore
pub struct Scope { /* tracks pending finalizers */ }

pub fn acquire_release<A, E, R>(
    acquire: Effect<A, E, R>,
    release: impl Fn(&A) -> Effect<(), Never, R>,
) -> Effect<A, E, R::WithScope>;
```

A `Scope` runs every registered finalizer when it closes, even if the
program errored, was interrupted, or panicked — in LIFO order, like
`Drop`, but `async`.

## The shape of the API (Phase 1 sketch)

```rust,ignore
use effect::{Effect, Scope};

let program = Effect::scoped(|scope: &Scope| async move {
    let conn = open_connection().acquire_in(scope).await?;
    let txn  = conn.begin_txn().acquire_in(scope).await?;
    do_work(&txn).await?;
    txn.commit().await
});
// On exit (success, failure, or interruption), commit/rollback the txn,
// then close the connection — in order.
```

This is the Rust equivalent of:

```ts
Effect.gen(function* () {
  const conn = yield* Effect.acquireRelease(openConnection, closeConnection)
  const txn  = yield* Effect.acquireRelease(beginTxn(conn), commitOrRollback)
  yield* doWork(txn)
})
```

When this lands the chapter is rewritten with runnable examples.
