# Quickstart

This walks through enough of `effect-rs` to build a small, testable program
end to end. We'll write a function that fetches a value, transforms it,
recovers from errors, and runs against an injected context.

## 1. Hello, Effect

```rust,no_run
use effect::Effect;

#[tokio::main]
async fn main() {
    let program = Effect::<_, String, ()>::succeed(20)
        .map(|x| x + 22)
        .tap(|x| println!("got {x}"));

    let answer = program.execute().await.unwrap();
    assert_eq!(answer, 42);
}
```

The three type parameters are the heart of effect-rs:

```rust,ignore
Effect<A, E, R>
//      │  │  │
//      │  │  └── the environment (services) required to run
//      │  └───── the typed failure
//      └──────── the successful result
```

`execute()` is only available when `R = ()` — meaning no environment is
required. For effects that need services, use `Runtime`.

## 2. Sequencing with `flat_map` or `Effect::block`

Two effects, each may fail:

```rust,no_run
use effect::Effect;

fn parse(s: &str) -> Effect<i32, String, ()> {
    let s = s.to_owned();
    Effect::sync(move || s.parse::<i32>().map_err(|e| e.to_string()))
}

fn double(x: i32) -> Effect<i32, String, ()> {
    Effect::succeed(x * 2)
}

// Combinator style
let combined = parse("21").flat_map(double);

// Do-notation style — Effect::block + `?` for short-circuiting.
// `g.run(eff).await?` runs the inner effect with the captured ctx and
// propagates Cause::Fail through `?`.
let do_style: Effect<i32, String, ()> = Effect::block(|g| async move {
    let parsed  = g.run(parse("21")).await?;
    let doubled = g.run(double(parsed)).await?;
    Ok(doubled)
});
```

Both produce `Effect<i32, String, ()>`. Pick the one that reads best:

- **Combinator chains** are tight when the work is linear.
- **`Effect::block`** is the standard for branching logic, multiple
  bindings, and anything you'd write as an `async fn`.

> `Effect::block` is named to avoid clashing with Rust 2024's reserved
> `gen` keyword. Effect-TS users will recognize it as the moral
> equivalent of `Effect.gen`.

## 3. Recovering from errors

```rust,no_run
use effect::Effect;

let safe = Effect::<i32, String, ()>::fail("nope".into())
    .catch_all(|_| Effect::succeed(0));   // → Effect<i32, String, ()>

let with_fallback = Effect::<i32, String, ()>::fail("nope".into())
    .or_else(Effect::succeed(0));
```

## 4. Providing services

The `R` parameter is where dependency injection lives. Define a trait,
make your effect generic over it, then provide an implementation at the
edge:

```rust,no_run
use effect::Effect;
use std::sync::Arc;

trait Logger: Send + Sync + 'static {
    fn log(&self, msg: &str);
}

struct ConsoleLogger;
impl Logger for ConsoleLogger {
    fn log(&self, msg: &str) { println!("{msg}"); }
}

fn announce<R: Logger>(msg: String) -> Effect<(), String, R> {
    Effect::from_fn(move |ctx: Arc<R>| {
        let msg = msg.clone();
        async move {
            ctx.log(&msg);
            Ok(())
        }
    })
}

#[tokio::main]
async fn main() -> Result<(), String> {
    announce::<ConsoleLogger>("hello".into())
        .provide(ConsoleLogger)
        .execute()
        .await
}
```

For more than one service, use a layered context — see
[Layers and the Runtime](../core/layers-and-runtime.md).

## 5. Concurrency with `zip`

```rust,no_run
use effect::Effect;

let a = Effect::<_, String, ()>::succeed(20);
let b = Effect::<_, String, ()>::succeed(22);

let pair = a.zip(b);                          // (20, 22)
let sum  = a.zip_with(b, |x, y| x + y);       // 42 — both run concurrently
```

## What to read next

- [The Effect Type](../core/the-effect-type.md) — every combinator on `Effect`.
- [Layers and the Runtime](../core/layers-and-runtime.md) — multi-service DI.
- [Todo Walkthrough](../examples/todo-walkthrough.md) — a full small app.
