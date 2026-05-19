# Design Decisions

Where `effect-rs` diverges from a literal port of Effect-TS, and why.

## Why a custom interpreter (Phase 1)

The Phase 0 prototype represents an `Effect` as `Arc<dyn Fn(Arc<R>) -> BoxFuture<Result<A, E>>>`.
This is the *simplest* thing that works, and it suffices for `succeed`,
`map`, `flat_map`, and `zip`. It does **not** suffice for:

- **Stack safety** — deep `flat_map` chains build a closure tree.
  Calling into it can blow the stack at `O(n)` depth.
- **Interruption** — closures own their futures; there's no place to
  insert a cooperative cancellation check.
- **Scopes** — finalizers must run *after* a future is dropped, even on
  abrupt termination. A free `Fn` has nowhere to register them.
- **Tracing the program tree** — for nice error reporting we want to know
  "you were inside `flat_map` step 3, retry #2".

Phase 1 replaces the internals with an instruction ADT (`Op<A, E, R>`)
and a trampolined runner that handles all four concerns. The public API
above the line stays the same.

## No HKT emulation

Several attempts exist in the Rust ecosystem (`fp-core`, `higher`) to
emulate higher-kinded types via GATs or associated types. They work, but
the ergonomics are poor and the error messages are atrocious.

Effect-TS *does* lean on HKT (the `* extends HKT` pattern) for its
typeclass hierarchy. We choose not to mirror that. Instead:

- Provide trait families (`Functor`-like, `Monad`-like) per-type.
- Provide *concrete* helpers (`Effect::map`, `Option::map`, `Stream::map`).
- Where mixed-type composition genuinely matters, provide a focused trait
  (e.g. `Pipeable`) using GATs.

The cost: no generic "traverse this `Functor`". The win: code that reads
like Rust and produces error messages a human can understand.

## Schema is *not* serde

`#[derive(Schema)]` will produce a schema descriptor at compile time, not
a serde codec. Reasons:

- Schemas are bidirectional and **transformable** — `parse` from JSON,
  `encode` to JSON, generate JSON Schema, generate CLI args, generate
  test data. Serde is unidirectional per-codec.
- Schemas support **refinements** — `Brand`, `Positive`, `MaxLength<100>`
  — that aren't just decoders.
- Schemas should compose with `Schema::transform` independent of
  representation.

We provide first-class **adapters** to serde so existing `Serialize` /
`Deserialize` types interoperate, but `Schema` is the source of truth.

## Trait-based Layers stay

The trait-bound layer approach in the prototype is the right Rust idiom:
the compiler does the dependency check, error messages name the missing
trait. Phase 1 keeps it and adds the standard `Layer` operators (`merge`,
`scoped`, failable construction) on top.

The alternative — a runtime `Context` keyed by `TypeId` (à la `anymap`)
— loses compile-time guarantees and produces worse error messages.

## tokio-first

We commit to `tokio` for the runtime in Phase 1–4. Reasons:

- `tracing`, `sqlx`, `hyper`, `reqwest` — every ecosystem crate we want
  to integrate with is tokio-shaped.
- Multi-runtime support is a tax on every feature; we'd rather pay it
  later when there's demonstrated demand.

When the time comes, the abstraction boundary is a small `RuntimeAdapter`
trait — we design for it but don't implement the alternative back-ends.
