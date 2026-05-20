# Streams

A [`Stream<A, E, R>`] is a description of an async producer of `A`
values that may fail with `E` and requires environment `R`. Defining a
stream is pure; running it (via a terminal like `run_collect` or
`run_for_each`) produces an [`Effect`].

Streams live in the `effect-stream` crate (which depends on `effect`,
so it can't be re-exported from the umbrella):

```toml
[dependencies]
effect = { version = "0.0.1" }
effect-stream = { version = "0.0.1" }
```

Under the hood we wrap [`futures::Stream`] — the de facto async stream
trait — so the futures ecosystem (combinators, IO adapters) interops
naturally where you need to drop down.

## A first pipeline

```rust,no_run
use effect_stream::Stream;

# #[tokio::main] async fn main() {
let evens_under_20: Stream<i32, String, ()> = Stream::from_iter(1..=10)
    .map(|x| x * 2)              // 2, 4, 6, …, 20
    .filter(|x| *x > 5)          // 6, 8, 10, …, 20
    .take(3);                    // 6, 8, 10

let result = evens_under_20.run_collect().execute().await;
assert_eq!(result.ok(), Some(vec![6, 8, 10]));
# }
```

## Constructors

| Constructor                        | Behavior                                              |
| ---------------------------------- | ----------------------------------------------------- |
| `Stream::empty()`                  | emits nothing                                         |
| `Stream::single(value)`            | one element                                           |
| `Stream::from_iter(iter)`          | every element from a `Clone`able iterable             |
| `Stream::fail(error)`              | immediately fails                                     |

## Transformations

All transforms return a new `Stream` (no I/O until you run it).

| Method                  | Shape                                                            |
| ----------------------- | ---------------------------------------------------------------- |
| `.map(f)`               | `(A) -> B`         — pure per-element                            |
| `.map_effect(f)`        | `(A) -> Effect<B, E, R>` — effectful per-element                 |
| `.filter(pred)`         | `(&A) -> bool`                                                   |
| `.take(n)`              | first N                                                          |
| `.drop(n)`              | skip first N                                                     |
| `.concat(other)`        | self first, then `other`                                         |

## Terminals — these produce `Effect`

| Method                       | Returns                                              |
| ---------------------------- | ---------------------------------------------------- |
| `.run_collect()`             | `Effect<Vec<A>, E, R>`                               |
| `.run_for_each(f)`           | `Effect<(), E, R>` — `f` is `Fn(A)`                  |
| `.run_drain()`               | `Effect<(), E, R>` — consume and discard             |
| `.run_fold(init, f)`         | `Effect<B, E, R>` — left fold                        |
| `.run_head()`                | `Effect<Option<A>, E, R>` — first item               |

```rust,no_run
use effect_stream::Stream;

# #[tokio::main] async fn main() {
let sum: i32 = Stream::<i32, String, ()>::from_iter(1..=4)
    .run_fold(0, |acc, x| acc + x)
    .execute()
    .await
    .ok()
    .unwrap();
assert_eq!(sum, 10);
# }
```

## Effects per item

`map_effect` is the main bridge to async I/O:

```rust,no_run
use effect::Effect;
use effect_stream::Stream;

# #[tokio::main] async fn main() {
# fn fetch_count(url: &str) -> Effect<usize, String, ()> {
#     Effect::succeed(url.len())
# }
let urls = vec!["a.com", "b.com", "c.com"];

let sizes = Stream::<&'static str, String, ()>::from_iter(urls)
    .map_effect(|u| fetch_count(u))
    .run_collect();

let total: Vec<usize> = sizes.execute().await.ok().unwrap();
# }
```

Items run **sequentially** through `map_effect`. For bounded
parallelism over a stream, the upcoming `map_effect_par(n, f)` will
mirror [`for_each_par`](./concurrency.md#for_each_par---bounded-parallel-map).

> Defects (`Cause::Die`) and interruption inside the per-item Effect
> currently **panic** when surfaced through `map_effect` — it uses
> `Exit::into_typed_result()` under the hood, which requires `E: Debug`
> and panics on non-`Fail` causes. A future `map_effect_exit` will
> hand the full `Exit` to the next stage.

## Stream + sink + channel

Today's `effect-stream` covers the **producer** side. Coming soon:

- **`Sink<A, E, R>`** — the dual: a consumer of `A` values, terminating
  with a result.
- **`Channel<A, E, R>`** — bidirectional pull-based pipe; the building
  block for streams that need to fan out.
- **`map_effect_par(n, f)`** — bounded-parallel per-element effects.
- **`merge`** / **`interleave`** — combine streams that race.
- **`group_within(n, duration)`** — batch into chunks of N or every D.
- **`throttle`** / **`debounce`** — rate-limiting.
- **`from_queue`** / **`into_queue`** — bridges to
  [`effect::Queue`](./state-and-sync.md#queue--async-mpmc-queue).
- **Resource-aware streams** — `Stream::scoped` that runs inside a
  scope so file/socket handles get cleaned up at end-of-stream.

## When to reach for streams

- **Unbounded or lazy data** — paging over an API response, tailing a
  file, processing a websocket.
- **Pipeline-shaped code** — when you'd otherwise reach for nested
  `for` loops + a result `Vec`, but want to start producing output
  before the input is fully read.
- **Back-pressured I/O** — combine `from_queue` (Phase 3a+) with a
  bounded queue and downstream consumers naturally back-pressure
  producers.

For one-shot work — fetch one thing, return it — stick with
[`Effect`](./the-effect-type.md). Streams pay for laziness.

[`Stream<A, E, R>`]: https://docs.rs/effect-stream/latest/effect_stream/struct.Stream.html
[`futures::Stream`]: https://docs.rs/futures/latest/futures/stream/trait.Stream.html
[`Effect`]: https://docs.rs/effect/latest/effect/struct.Effect.html
