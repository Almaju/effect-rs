# effect-rs

A functional effect framework for Rust, porting the design and ergonomics
of [Effect-TS](https://effect.website) to the Rust ecosystem.

`effect-rs` brings typed errors, typed dependencies, structured concurrency,
schemas, layers, and observability to async Rust — without sacrificing the
ownership and zero-cost abstractions that make Rust Rust.

```rust,no_run
use effect::Effect;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), String> {
    let program = Effect::<_, String, ()>::succeed(20)
        .map(|x| x + 22)
        .tap(|x| println!("got {x}"));

    let answer = program.execute().await?;
    assert_eq!(answer, 42);
    Ok(())
}
```

## Status

Pre-alpha. The core `Effect<A, E, R>`, basic combinators, and a trait-based
`Layer`/`Runtime` are in place; see the [roadmap](./project/roadmap.md) for
what's coming.

## Inspirations

- [Effect-TS](https://effect.website) — the direct inspiration
- [ZIO](https://zio.dev) — Effect-TS's own inspiration
- Haskell's `mtl` / `freer-simple`, PureScript's `Run`, `fp-ts`

## Why a Rust port?

Effect changed how a lot of people write TypeScript: typed errors became
real, dependency injection became compile-checked, and async became
composable. Rust already has many ingredients (algebraic data types,
`Result`, ownership, real newtypes), but it leans heavy on `?` and trait
objects when you want layered services or transactional pipelines.
`effect-rs` aims to give Rust users the same one-framework experience for
building testable, effectful, async programs.
