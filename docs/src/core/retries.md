# Retries and Schedules

A [`Schedule`] is a *pure value* describing when (and whether) to repeat
something. It's a state machine: each `step()` either yields
`Continue(delay, next_schedule)` or `Done`. State lives in the captured
closure of the next schedule — composition is just wrapping.

Pair a schedule with [`Effect::retry`] (for failure) or
[`Effect::repeat`] (for success) and you get policy-driven recovery and
polling with one line of code.

## A first retry

```rust,no_run
use effect::{Effect, Schedule};
use std::time::Duration;

# #[tokio::main] async fn main() {
let flaky = Effect::<i32, String, ()>::sync(|| {
    if rand::random::<bool>() { Ok(42) } else { Err("transient".into()) }
});

let robust = flaky.retry(
    Schedule::exponential(Duration::from_millis(50)).max_attempts(5)
);
# }
```

`retry` only fires on `Cause::Fail`. Defects (`Die`) and interruption
are not retried — those are bugs or structural events, not transient
errors. If you genuinely want to retry through everything, sandbox first:

```rust,no_run
use effect::{Effect, Schedule};
let _ = Effect::<i32, String, ()>::succeed(0)
    .sandbox()
    .retry(Schedule::recurs(3))
    .unsandbox();
```

## Polling with `repeat`

`repeat` runs the effect, and on success consults the schedule for whether
to run again. The *last* success value is returned. A failure at any
iteration aborts the loop and surfaces immediately.

```rust,no_run
use effect::{Effect, Schedule};
use std::time::Duration;

# #[tokio::main] async fn main() {
let poll = Effect::<i32, String, ()>::sync(|| Ok(42));

// Poll every 100ms for up to 5 seconds.
let runner = poll.repeat(
    Schedule::spaced(Duration::from_millis(100))
        .bounded(Duration::from_secs(5)),
);
# }
```

## Schedule combinators

| Constructor                  | Behavior                                              |
| ---------------------------- | ----------------------------------------------------- |
| `Schedule::stop()`           | Stop immediately.                                     |
| `Schedule::once()`           | One continue, no delay, then stop.                    |
| `Schedule::forever()`        | Continue forever, no delay.                           |
| `Schedule::recurs(n)`        | Continue up to `n` times, no delay.                   |
| `Schedule::spaced(d)`        | Continue forever with fixed `d` between iterations.   |
| `Schedule::exponential(b)`   | Continue forever, delays doubling from `b`.           |
| `Schedule::fibonacci(b)`     | Continue forever, Fibonacci-spaced from `b`.          |

| Combinator                          | Behavior                                          |
| ----------------------------------- | ------------------------------------------------- |
| `.max_attempts(n)`                  | Cap any schedule at `n` iterations.               |
| `.bounded(max_total)`               | Stop once cumulative delay would exceed budget.   |

> `jittered` (random jitter on each delay) and composition combinators
> (`andThen`, `intersect`, `union`, `whileInput`) are planned for the
> next Schedule pass. The current set covers the common
> retry-with-backoff / poll-with-budget cases.

## Putting it together

A typical resilient remote call:

```rust,no_run
use effect::{Effect, Schedule};
use std::time::Duration;

# fn fetch_user() -> Effect<u64, String, ()> { Effect::succeed(1) }
# #[tokio::main] async fn main() {
let fetched = fetch_user()
    .retry(
        Schedule::exponential(Duration::from_millis(100))
            .max_attempts(4)
            .bounded(Duration::from_secs(10)),
    );
# }
```

That schedule says: *retry on typed failure, with delays starting at
100 ms and doubling each time, but no more than 4 retries and no longer
than 10 s of cumulative wait.*

## Counting attempts

`Schedule` itself doesn't expose iteration count today. If you need the
count, use a `Ref` alongside:

```rust,no_run
use effect::{Effect, Ref, Schedule};
use std::time::Duration;

# #[tokio::main] async fn main() {
let attempt_count = Ref::new(0_u64);
let counter = attempt_count.clone();
let flaky = Effect::<i32, String, ()>::sync(move || {
    let _ = counter.update::<String, (), _>(|n| n + 1);
    Err("nope".into())
});

let _ = flaky
    .retry(Schedule::recurs(3))
    .execute()
    .await;

let n = attempt_count.get::<String, ()>().execute().await.ok();
assert_eq!(n, Some(4));   // initial + 3 retries
# }
```

A future Schedule pass will expose iteration count and elapsed time as
outputs without needing an external `Ref`.

## What's coming

- `Schedule::jittered(factor)` — multiply each delay by a random factor
  in `[1 - factor, 1 + factor]` to spread thundering-herd retries.
- Composition: `andThen`, `intersect` (max-delay), `union` (min-delay).
- Predicate-driven schedules: `whileInput`, `whileOutput`, `passing`.
- Schedule-aware retry: `retry_with_state` exposing the schedule's
  output (count, elapsed) to the handler.

[`Schedule`]: https://docs.rs/effect/latest/effect/schedule/struct.Schedule.html
[`Effect::retry`]: https://docs.rs/effect/latest/effect/struct.Effect.html#method.retry
[`Effect::repeat`]: https://docs.rs/effect/latest/effect/struct.Effect.html#method.repeat
