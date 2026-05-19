# Concurrency

Two structural combinators today, plus the
[state-and-synchronization primitives](./state-and-sync.md) — `Ref`,
`Deferred`, `Queue`. Many more after Phase 1's interpreter lands.

## Available now

### `zip` / `zip_with`

Run two effects concurrently via `tokio::join!`, collecting both results.
If either errors, the joined effect errors.

```rust,no_run
use effect::Effect;

let pair = Effect::<_, String, ()>::succeed(20)
    .zip(Effect::succeed(22));               // → (20, 22)

let sum  = Effect::<_, String, ()>::succeed(20)
    .zip_with(Effect::succeed(22), |a, b| a + b);   // → 42
```

The two effects share the environment. They run on the same tokio runtime
and yield concurrently — there is no spawning.

## Coming in Phase 1

### `fork` — spawn a fiber

```rust,ignore
let fiber: Fiber<A, E> = work().fork();   // detaches; returns a handle
let result: A          = fiber.join().await?;
fiber.interrupt().await;                  // cooperative cancellation
```

A `Fiber` is `effect-rs`'s equivalent of a green thread, modeled on
tokio's `JoinHandle` but with **supervised cancellation**: if the parent
fiber dies, all its children receive `Interrupt`.

### `race`

```rust,ignore
let first: A = work_a().race(work_b()).await?;
```

Returns the first to succeed; cancels the loser.

### `for_each_par` / `collect_par`

```rust,ignore
let results = inputs.for_each_par(8, |x| process(x)).await?;
```

Bounded-parallelism mapping — equivalent to Effect-TS's
`Effect.forEach(items, f, { concurrency: 8 })`.

### `interrupt` / `uninterruptible` / `on_interrupt`

Cooperative cancellation primitives. Critical sections wrap themselves
in `.uninterruptible()`; cleanup hooks register via `.on_interrupt(…)`.

This chapter expands once the interpreter lands.
