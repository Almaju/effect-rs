# State and Synchronization

Four primitives for sharing state and coordinating across async tasks:

| Primitive          | Shape                                          | When to reach for it                                  |
| ------------------ | ---------------------------------------------- | ----------------------------------------------------- |
| [`Ref<A>`]         | A mutable cell.                                | Counters, caches, anywhere you'd reach for `Arc<Mutex<A>>`. |
| [`Deferred<A, E>`] | A one-shot promise of an `Exit<A, E>`.         | "Compute once, await many times" — initialization, gate. |
| [`Queue<A>`]       | MPMC async queue, bounded or unbounded.         | Fan-in / fan-out, work pipelines, back-pressured streams. |
| [`Semaphore`]      | Counting semaphore.                             | Concurrency limits — "at most N of these at once".    |

All three are cheap to `clone()` — clones share the same underlying
state via `Arc`. Hand them to any number of tasks; operations are
effect-typed so they slot into any `flat_map`/`from_fn` chain.

## `Ref` — shared mutable state

```rust,no_run
use effect::Ref;

# #[tokio::main] async fn main() {
let counter = Ref::new(0_i32);

counter.update::<String, (), _>(|n| n + 1).execute().await;
counter.update::<String, (), _>(|n| n + 1).execute().await;
counter.update::<String, (), _>(|n| n + 1).execute().await;

let value = counter.get::<String, ()>().execute().await.ok();
assert_eq!(value, Some(3));
# }
```

The turbofish (`::<String, ()>`) appears in standalone calls because
`Ref`'s ops are generic over the surrounding `E` and `R` — making them
slot into any context. Inside a `flat_map` chain the bounds are inferred:

```rust,no_run
use effect::{Effect, Ref};

# #[tokio::main] async fn main() {
let counter = Ref::new(0);
let program = counter.update::<String, (), _>(|n| n + 1)
    .flat_map({
        let c = counter.clone();
        move |_| c.get()
    });

assert_eq!(program.execute().await.ok(), Some(1));
# }
```

### Available ops

| Method              | Returns                  | Notes                                       |
| ------------------- | ------------------------ | ------------------------------------------- |
| `get()`             | `Effect<A, E, R>`        | Reads the current value (clones).           |
| `set(a)`            | `Effect<(), E, R>`       | Overwrites unconditionally.                 |
| `update(f)`         | `Effect<(), E, R>`       | `f: Fn(A) -> A` — read-modify-write.         |
| `modify(f)`         | `Effect<B, E, R>`        | `f: Fn(A) -> (B, A)` — output + new value.   |
| `get_and_set(a)`    | `Effect<A, E, R>`        | Returns old value; stores new.              |

### Concurrent access is safe

```rust,no_run
use effect::Ref;

# #[tokio::main] async fn main() {
let counter = Ref::new(0_i32);
let mut handles = Vec::new();
for _ in 0..100 {
    let c = counter.clone();
    handles.push(tokio::spawn(async move {
        c.update::<String, (), _>(|n| n + 1).execute().await;
    }));
}
for h in handles { h.await.unwrap(); }
assert_eq!(counter.get::<String, ()>().execute().await.ok(), Some(100));
# }
```

Internally `Ref` is `Arc<Mutex<A>>`. Locks are released between
operations, so there's no risk of deadlocking across awaits.

## `Deferred` — one-shot promise

A `Deferred<A, E>` starts empty. Exactly one completer wins (the first
`succeed`/`fail`/`complete`); subsequent completions are no-ops. Any
number of awaiters suspend on it and all receive the same outcome when
it lands.

```rust,no_run
use effect::Deferred;
use std::time::Duration;

# #[tokio::main] async fn main() {
let d: Deferred<i32, String> = Deferred::new();

// Producer task
let writer = d.clone();
tokio::spawn(async move {
    tokio::time::sleep(Duration::from_millis(50)).await;
    writer.succeed::<String, ()>(42).execute().await;
});

// Consumer (anywhere)
let got = d.await_::<()>().execute().await;
assert_eq!(got.ok(), Some(42));
# }
```

### When to use it

- **One-shot initialization:** "wait until the connection pool is ready".
- **Coordination gate:** "release N workers once the manifest is loaded".
- **Capturing a fiber result without `flat_map`:** spawn work, then
  `await_()` from anywhere holding the deferred.

### Available ops

| Method              | Returns                              | Notes                                              |
| ------------------- | ------------------------------------ | -------------------------------------------------- |
| `succeed(a)`        | `Effect<bool, E2, R>`                | `true` if this call won the race.                  |
| `fail(e)`           | `Effect<bool, E2, R>`                | Completes with `Cause::Fail(e)`.                   |
| `complete(exit)`    | `Effect<bool, E2, R>`                | Set any `Exit` (including `Die`).                  |
| `await_()`          | `Effect<A, E, R>`                    | Suspends until completed; replays the `Exit`.      |
| `poll()`            | `Effect<Option<Exit<A, E>>, E2, R>`  | Non-blocking snapshot.                             |
| `is_done()`         | `Effect<bool, E2, R>`                | Non-blocking.                                      |

The completer chooses what `Exit` to publish (success, typed failure,
defect). Awaiters see exactly that — so a `Deferred` carrying a `Die`
will replay the defect to every awaiter.

## `Queue` — async MPMC queue

Backed by [`async-channel`]. Both ends are clone-able, so any number of
producers and consumers can share the queue.

```rust,no_run
use effect::Queue;

# #[tokio::main] async fn main() {
let q: Queue<i32> = Queue::new_unbounded();

// Producers
for i in 0..5 {
    q.offer::<String, ()>(i).execute().await;
}

// Consumers
let mut taken = Vec::new();
while let Some(v) = q.take::<String, ()>().execute().await.ok().flatten() {
    taken.push(v);
    if taken.len() == 5 { break; }
}
assert_eq!(taken, vec![0, 1, 2, 3, 4]);
# }
```

### Bounded vs unbounded

```rust,no_run
use effect::Queue;
let _ = Queue::<i32>::new_unbounded();      // never back-pressures
let _ = Queue::<i32>::new_bounded(16);      // offer suspends when full
```

`offer` on a bounded queue suspends until space is available. Use
`try_offer` if you want a non-blocking attempt that returns `false`
when full.

### Shutdown

```rust,no_run
use effect::Queue;

# #[tokio::main] async fn main() {
let q: Queue<i32> = Queue::new_unbounded();
q.offer::<String, ()>(1).execute().await;
q.offer::<String, ()>(2).execute().await;
q.shutdown::<String, ()>().execute().await;

// Buffered items drain:
assert_eq!(q.take::<String, ()>().execute().await.ok().flatten(), Some(1));
assert_eq!(q.take::<String, ()>().execute().await.ok().flatten(), Some(2));
// Then `take` returns None:
assert_eq!(q.take::<String, ()>().execute().await.ok().flatten(), None);
// And further offers are rejected:
assert_eq!(q.offer::<String, ()>(3).execute().await.ok(), Some(false));
# }
```

### Available ops

| Method              | Returns                          | Notes                                            |
| ------------------- | -------------------------------- | ------------------------------------------------ |
| `offer(a)`          | `Effect<bool, E, R>`             | Suspends on bounded full; `false` if shut down.  |
| `try_offer(a)`      | `Effect<bool, E, R>`             | Non-blocking; `false` if full or shut down.       |
| `take()`            | `Effect<Option<A>, E, R>`        | Suspends; `None` once drained-and-shut-down.     |
| `try_take()`        | `Effect<Option<A>, E, R>`        | Non-blocking; `None` if empty.                    |
| `shutdown()`        | `Effect<(), E, R>`               | Closes the queue.                                |
| `size()`            | `Effect<usize, E, R>`            | Buffered item count.                             |
| `is_empty()`        | `Effect<bool, E, R>`             |                                                  |

## `Semaphore` — bound concurrency

A counting semaphore: at most *N* effects may hold a permit at once. The
typical entry point is `with_permit`, which acquires, runs, and
releases — automatically, even on failure.

```rust,no_run
use effect::{Effect, Semaphore};
use std::time::Duration;

# #[tokio::main] async fn main() {
let sem = Semaphore::new(2);   // at most 2 concurrent calls

let work = |i: i32| Effect::<i32, String, ()>::from_fn(move |_| async move {
    tokio::time::sleep(Duration::from_millis(50)).await;
    Ok(i * 2)
});

// Launch 10 jobs but only 2 ever run concurrently.
let mut handles = Vec::new();
for i in 0..10 {
    let sem = sem.clone();
    let w = work(i);
    handles.push(tokio::spawn(async move {
        sem.with_permit(w).execute().await
    }));
}
for h in handles { let _ = h.await.unwrap(); }
# }
```

### Available ops

| Method                  | Returns                          | Notes                                              |
| ----------------------- | -------------------------------- | -------------------------------------------------- |
| `available_permits()`   | `Effect<usize, E, R>`            |                                                    |
| `try_acquire()`         | `Effect<bool, E, R>`             | Probe (releases immediately).                       |
| `with_permit(eff)`      | `Effect<A, E, R>`                | Run `eff` holding one permit.                       |
| `with_permits(n, eff)`  | `Effect<A, E, R>`                | Run `eff` holding `n` permits at once.              |
| `close()`               | `Effect<(), E, R>`               | Subsequent acquires fail with `Cause::Die`.        |

Permits are released on the way out — success, typed failure, or
defect. There's no risk of leaking a permit by forgetting to release.

## What's coming

- **PubSub** — broadcast with subscriber back-pressure.
- **`scoped`-built primitives** — once `Scope` lands, queues and refs
  can be auto-shut-down at scope end.
- **STM (`TxRef`, `TxQueue`, …)** — Phase 3.
- **`eff!` macro** — drops the `::<String, ()>` turbofishes when the
  surrounding chain pins the types.

[`async-channel`]: https://docs.rs/async-channel
[`Semaphore`]: https://docs.rs/effect/latest/effect/sync/semaphore/struct.Semaphore.html
