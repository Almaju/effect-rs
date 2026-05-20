# Concurrency

`effect-rs` offers four ways to express concurrency, in rough order of
"how heavyweight":

| Tool                        | When                                             |
| --------------------------- | ------------------------------------------------ |
| `zip` / `zip_with`          | Two effects, both must complete, share the task. |
| `race`                      | Two effects, want the first to finish.           |
| `fork` + `Fiber::join`      | Need a handle for explicit waiting / interrupting. |
| The [sync primitives](./state-and-sync.md) | Coordinate by shared state. |

All of these compose with [cooperative cancellation](#interruption) and
[scopes](./scopes-and-resources.md).

## `zip` / `zip_with` — two-and-go

```rust,no_run
use effect::Effect;

# #[tokio::main] async fn main() {
let pair = Effect::<_, String, ()>::succeed(20)
    .zip(Effect::succeed(22));

let sum  = Effect::<_, String, ()>::succeed(20)
    .zip_with(Effect::succeed(22), |a, b| a + b);
# }
```

Internally `tokio::join!` — no spawning, both arms run on the same task.
If both arms fail concurrently, causes combine as `Cause::Parallel`.

## `race` — first to finish wins

```rust,no_run
use effect::Effect;

# #[tokio::main] async fn main() {
let fast = Effect::<_, String, ()>::from_fn(|_| async {
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    Ok(1)
});
let slow = Effect::<_, String, ()>::from_fn(|_| async {
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    Ok(2)
});

let winner = fast.race(slow);   // Effect<i32, String, ()>
# }
```

`race` spawns both arms as fibers, awaits whichever completes first
(success **or** typed failure), and interrupts the loser. If you only
want to keep the first success, chain a `.or_else(...)`.

## `fork` + `Fiber` — explicit handles

```rust,no_run
use effect::Effect;

# #[tokio::main] async fn main() {
let work = Effect::<i32, String, ()>::from_fn(|_| async { Ok(42) });

let program = Effect::<_, String, ()>::block(move |g| {
    let work = work.clone();
    async move {
        let fiber = g.run(work.fork()).await?;     // spawned, runs in background
        // … do other stuff …
        let exit = fiber.join().await;             // Exit<i32, String>
        Ok::<i32, String>(exit.ok().unwrap())
    }
});
# }
```

`Effect::fork(self)` returns `Effect<Fiber<A, E>, E, R>`. Running it
spawns the inner effect on a new `tokio::spawn` task with its own
[`FiberState`], and produces a `Fiber<A, E>` handle. The outer never
fails — the failure (if any) is inside the fiber.

`Fiber::join().await` waits for completion, returning `Exit<A, E>`:
- a tokio task panic becomes `Cause::Die`,
- a tokio task cancellation becomes `Cause::Interrupt`,
- normal completion forwards the inner `Exit`.

### Interrupting a fiber

```rust,no_run
# use effect::Effect;
# #[tokio::main] async fn main() {
# let work = Effect::<(), String, ()>::succeed(());
let program = Effect::<_, String, ()>::block(move |g| {
    let work = work.clone();
    async move {
        let fiber = g.run(work.fork()).await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        fiber.interrupt();          // sets the child's interrupt flag
        let _ = fiber.join().await; // child observes at next checkpoint
        Ok::<_, String>(())
    }
});
# }
```

`Fiber::interrupt` sets the child's interrupt flag (an `Arc<AtomicBool>`
shared with the child's `FiberState`). The child observes it at the
**next `flat_map` boundary** and short-circuits with
`Cause::Interrupt` — unless that boundary lives inside an
`.uninterruptible()` region.

## Interruption

The interrupt flag is per-fiber and per-execution: each `Effect::run`
sets up a fresh one. Constructors and combinators:

| API                              | Effect                                        |
| -------------------------------- | --------------------------------------------- |
| `Effect::interrupt()`            | Constructor that fails with `Cause::Interrupt` immediately. |
| `effect.interruptible()`         | Mark a region as interruptible (the default). |
| `effect.uninterruptible()`       | Mark a region uninterruptible: pending interrupt observed via `FiberState::is_interrupted` but `flat_map` boundaries don't short-circuit. |
| `Fiber::interrupt()`             | Set a child fiber's flag from outside.        |

Interrupt regions nest lexically — the **innermost** wins. So
`.interruptible()` inside an `.uninterruptible()` re-enables short-
circuiting for that inner region. Critical sections (writes that must
not be torn down) wrap themselves in `.uninterruptible()`; cleanup
done via [scope finalizers](./scopes-and-resources.md) always runs
uninterruptibly so it can't be cancelled mid-flight.

## What's coming

- **`Effect::for_each_par(n, f)`** — bounded-parallelism mapping over an
  iterable, equivalent to Effect-TS's `Effect.forEach(items, f, { concurrency: n })`.
- **`Effect::race_all([…])`** — N-way race instead of pairwise.
- **`Fiber::join_with_finalizer`** — join under a scope so the joined
  fiber's resources get cleaned up automatically.
- **`Effect::on_interrupt(self, handler)`** — register a handler that
  fires only when the effect is interrupted (not on success or typed
  failure).
- **Supervised fork** — `fork` variants where the parent's exit
  automatically interrupts every child it started.
