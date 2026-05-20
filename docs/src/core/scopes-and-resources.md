# Scopes and Resources

Rust's `Drop` is great for synchronous, owned resources. It's not
enough when:

- The resource is acquired asynchronously (`async fn connect()`).
- Release itself is asynchronous (`async fn close()`).
- The resource needs to outlive a specific function call but die when
  its enclosing scope does.
- A parent computation's cancellation must roll back every resource
  its children acquired — *and* run the cleanup even mid-cancel.

`Scope` + `acquire_release` is the answer.

## The shape

```rust,no_run
use effect::{Effect, acquire_release};

# async fn open_connection() -> Result<Conn, String> { todo!() }
# async fn close_connection(_: Conn) {}
# struct Conn;
# impl Clone for Conn { fn clone(&self) -> Self { Conn } }
# unsafe impl Send for Conn {} unsafe impl Sync for Conn {}
# fn use_connection(_c: &Conn) -> Effect<(), String, ()> { Effect::succeed(()) }
# #[tokio::main] async fn main() {
let program = Effect::<(), String, ()>::scoped(
    acquire_release(
        Effect::from_fn(|_| async { open_connection().await }),
        |conn| async move { close_connection(conn).await },
    )
    .flat_map(|conn| use_connection(&conn)),
);

program.execute().await;
# }
```

When `program` finishes — for any reason — every registered finalizer
runs in **LIFO** order before the outer effect returns.

## Failure modes — finalizers always run

| Outcome of the body          | Finalizers run? |
| ---------------------------- | --------------- |
| `Exit::Success(_)`           | yes             |
| `Cause::Fail(_)` (typed)     | yes             |
| `Cause::Die(_)` (defect)     | yes             |
| `Cause::Interrupt`           | yes             |

```rust,no_run
use effect::{Effect, Cause, Exit, acquire_release};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

# #[tokio::main] async fn main() {
let released = Arc::new(AtomicUsize::new(0));
let released_clone = released.clone();

let body = Effect::<i32, String, ()>::scoped(
    acquire_release(
        Effect::sync(|| Ok(42)),
        move |_| {
            let released = released_clone.clone();
            async move { released.fetch_add(1, Ordering::SeqCst); }
        },
    )
    .flat_map(|_| Effect::<i32, String, ()>::die_message("boom")),
);

let exit = body.execute().await;
assert!(matches!(exit, Exit::Failure(Cause::Die(_))));
assert_eq!(released.load(Ordering::SeqCst), 1);    // released anyway
# }
```

Finalizers themselves run **uninterruptibly** — a pending interrupt
can't tear down a connection mid-cleanup.

## LIFO order

```rust,no_run
# use effect::{Effect, acquire_release};
# #[tokio::main] async fn main() {
let program = Effect::<(), String, ()>::scoped(
    acquire_release(Effect::sync(|| Ok(())), |_| async { println!("close A"); })
        .flat_map(|_| acquire_release(Effect::sync(|| Ok(())), |_| async { println!("close B"); }))
        .flat_map(|_| acquire_release(Effect::sync(|| Ok(())), |_| async { println!("close C"); }))
        .void(),
);
program.execute().await;
//   close C
//   close B
//   close A
# }
```

This matches the structure of `Drop` for synchronous resources: the
last thing acquired is the first thing released.

## What goes wrong without a scope

`acquire_release` called outside an `Effect::scoped` region panics —
"called outside an Effect::scoped region". The panic is caught into
`Cause::Die` (because the registration runs inside `Effect::sync`),
making it loud but not fatal:

```rust,no_run
use effect::{Effect, Cause, Exit, acquire_release};

# #[tokio::main] async fn main() {
let oops = acquire_release(
    Effect::<i32, String, ()>::sync(|| Ok(42)),
    |_| async { /* cleanup */ },
);
let exit = oops.execute().await;   // no Effect::scoped wrapping
match exit {
    Exit::Failure(Cause::Die(d)) => assert!(d.message.contains("outside")),
    _ => unreachable!(),
}
# }
```

Effect-TS encodes this as a type-level `Scope` requirement on `R`.
Rust can't easily express that, so we use a runtime check with a
loud error message instead.

## Composing scopes

A `scoped` region can itself contain another `scoped` region —
finalizers belong to the closest enclosing one and run when that
scope closes:

```rust,ignore
Effect::scoped(
    acquire_release(open_outer, close_outer)
        .flat_map(|outer| Effect::scoped(
            acquire_release(open_inner, close_inner)
                .flat_map(|inner| do_work(outer, inner))
        ))
);
// Order at exit:
//   close_inner   (inner scope closes first)
//   close_outer
```

## What's coming

- `Effect::ensuring(self, finalizer)` — convenience for "do this work,
  then this cleanup regardless of outcome", without needing
  `acquire_release` framing.
- `Effect::on_interrupt(self, handler)` — fire a finalizer only on
  cancellation.
- `Effect::add_finalizer(eff)` — explicit registration from within a
  scope.
- Scope-aware `Layer` (Phase 1+): layers that acquire/release at
  Runtime boundaries.
