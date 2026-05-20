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

### `fork_scoped` — child tied to surrounding scope

```rust,no_run
use effect::Effect;

# #[tokio::main] async fn main() {
# let background = Effect::<(), String, ()>::succeed(());
# let work = Effect::<i32, String, ()>::succeed(0);
let program = Effect::<i32, String, ()>::scoped(
    Effect::<_, String, ()>::block(move |g| {
        let bg = background.clone();
        let work = work.clone();
        async move {
            let _bg_fiber = g.run(bg.fork_scoped()).await?;   // dies with scope
            g.run(work).await
        }
    })
);
# }
```

`fork_scoped` registers an interrupt-on-scope-close finalizer for the
child. When the surrounding `Effect::scoped` closes — for any reason —
the child gets its flag set. Useful for "start a background helper for
the lifetime of this scope".

Outside a `scoped` region, `fork_scoped` fails with `Cause::Die`.

## `for_each_par` — bounded parallel map

```rust,no_run
use effect::{Effect, for_each_par};

# #[tokio::main] async fn main() {
let urls = vec!["a", "b", "c", "d", "e"];

let program: Effect<Vec<String>, String, ()> = for_each_par(urls, 3, |url| {
    Effect::from_fn(move |_| async move {
        // ... fetch ...
        Ok(format!("fetched {url}"))
    })
});

let results = program.execute().await.ok().unwrap();
assert_eq!(results.len(), 5);
# }
```

- At most `concurrency` effects run at once (via an internal semaphore).
- Results are returned in **input order**.
- All children **share the parent's interrupt flag** — any failure
  signals the rest to short-circuit; on first observed failure the
  result is returned and in-flight tasks become orphans.
- `concurrency` is clamped to at least 1.

## `on_interrupt` — handle pure cancellation

```rust,no_run
use effect::Effect;

# #[tokio::main] async fn main() {
# let main_work = Effect::<i32, String, ()>::succeed(0);
let program = main_work.on_interrupt(|| async {
    eprintln!("interrupted — logging it");
});
# }
```

Fires only when the effect's cause is a **pure interrupt** —
`Cause::Interrupt` or a compound made up entirely of `Interrupt`
leaves. Typed failures, defects, and successful runs don't trigger it.
The handler runs uninterruptibly so subsequent cancellation can't
prevent its cleanup.

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

- **`Effect::race_all([…])`** — N-way race instead of pairwise.
- **`Effect::join_par(effects)`** — like `for_each_par` but for a
  fixed-shape tuple of heterogeneous effects.
- **`Fiber::interrupt_and_join`** — `interrupt(); join().await` shorthand
  that returns the child's eventual `Exit`.
- **Supervised fork** — `fork` variants where the parent's exit
  automatically interrupts every child it started (subsumes
  `fork_scoped` once the type-level Scope tracking lands).
