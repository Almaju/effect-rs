# Platform Services

`effect-platform` exposes three commonly-mocked side channels as
injectable services:

| Service       | Reads / writes…                                    |
| ------------- | -------------------------------------------------- |
| `Clock`       | wall-clock time, monotonic instants                |
| `Random`      | non-crypto random ints/floats/bytes (via `fastrand`)|
| `FileSystem`  | read/write/exists on the local FS (via `tokio::fs`)|

Each is a trait you bound `R` by, paired with a `Live*` impl that hits
the OS. In tests you swap in a fake; in production you wire the
`Live*` impls into your context.

```toml
[dependencies]
effect          = { version = "0.0.1" }
effect-platform = { version = "0.0.1" }
```

## `Clock`

```rust,no_run
use effect::Effect;
use effect_platform::{clock, Clock, LiveClock};
use std::time::Duration;

# #[tokio::main] async fn main() {
let program: Effect<_, String, LiveClock> = Effect::block(|g| async move {
    let started = g.run(clock::monotonic::<String, LiveClock>()).await?;
    g.run(clock::sleep::<String, LiveClock>(Duration::from_millis(20))).await?;
    let elapsed = g.run(clock::monotonic::<String, LiveClock>()).await?.duration_since(started);
    Ok::<_, String>(format!("ran for {elapsed:?}"))
});

let _ = program.run_with(LiveClock).await;
# }
```

`clock::sleep` doesn't actually require a `Clock` (it drives tokio's
timer directly) — but mocking the clock in tests still lets you
freeze "now":

```rust,no_run
use effect_platform::Clock;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

pub struct FakeClock { pub instant: Mutex<Instant> }
impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1)
    }
    fn monotonic(&self) -> Instant { *self.instant.lock().unwrap() }
}
```

## `Random`

```rust,no_run
use effect::Effect;
use effect_platform::{random, LiveRandom};

# #[tokio::main] async fn main() {
let coin: Effect<u64, String, LiveRandom> = random::range_u64(2);
let result = coin.run_with(LiveRandom).await;
# }
```

| Function                  | Returns                                |
| ------------------------- | -------------------------------------- |
| `random::next_u64()`      | `Effect<u64, E, R: Random>`            |
| `random::next_f64()`      | `Effect<f64, E, R: Random>`            |
| `random::range_u64(n)`    | `Effect<u64, E, R: Random>` in `[0, n)`|
| `random::fill_bytes(n)`   | `Effect<Vec<u8>, E, R: Random>`        |

`LiveRandom` uses [`fastrand`] — fast, non-cryptographic. For
crypto-grade randomness, plug in your own `Random` impl backed by
e.g. `ring::rand::SystemRandom`.

## `FileSystem`

```rust,no_run
use effect::Effect;
use effect_platform::{fs, FileSystem, LiveFileSystem};

# #[tokio::main] async fn main() {
let program: Effect<String, std::io::Error, LiveFileSystem> = Effect::block(|g| async move {
    g.run(fs::write::<LiveFileSystem>("/tmp/hi.txt", "hello".as_bytes().to_vec())).await?;
    g.run(fs::read_to_string::<LiveFileSystem>("/tmp/hi.txt")).await
});
let _ = program.run_with(LiveFileSystem).await;
# }
```

Method shapes return `AsyncResult<io::Result<T>>` (a boxed future) so
the trait stays object-safe.

| Function                  | Returns                                              |
| ------------------------- | ---------------------------------------------------- |
| `fs::read_to_string(p)`   | `Effect<String, io::Error, R: FileSystem>`           |
| `fs::write(p, bytes)`     | `Effect<(), io::Error, R: FileSystem>`               |
| `fs::exists(p)`           | `Effect<bool, E, R: FileSystem>` (never errors)      |

## Mocking in tests

The whole point of platform-as-service is testability. The pattern
mirrors the [Layers and Runtime](./layers-and-runtime.md) chapter:

```rust,no_run
# use effect_platform::FileSystem;
# use std::collections::HashMap;
# use std::path::PathBuf;
# use std::sync::Mutex;
pub struct InMemoryFs(Mutex<HashMap<PathBuf, Vec<u8>>>);

impl FileSystem for InMemoryFs {
    fn read_to_string(&self, path: PathBuf) -> effect_platform::filesystem::AsyncResult<std::io::Result<String>> {
        let map = self.0.lock().unwrap();
        let result = map.get(&path)
            .ok_or(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"))
            .map(|bytes| String::from_utf8_lossy(bytes).to_string());
        Box::pin(async move { result })
    }
    // … write, exists similarly …
#   fn write(&self, _: PathBuf, _: Vec<u8>) -> effect_platform::filesystem::AsyncResult<std::io::Result<()>> { Box::pin(async { Ok(()) }) }
#   fn exists(&self, _: PathBuf) -> effect_platform::filesystem::AsyncResult<bool> { Box::pin(async { false }) }
}
```

Now your business logic — `R: FileSystem`-bounded — runs identically
against real disk or this in-memory fake.

## What's coming

- **`Terminal`** — read line / write line, color attributes.
- **`Stdio`** — explicit stdin / stdout / stderr handles for
  capture-in-tests.
- **`Process`** / **`Command`** — spawn external programs, capture
  output, signal/kill semantics.
- **`Path`** — typed path manipulation effects (currently rely on
  `std::path::Path`).
- **Crypto-grade Random** — `SecureRandom` trait + ring/rand-based
  impl.
- **Tokio-FS bridges** — `read_dir`, `metadata`, `create_dir_all`.

[`fastrand`]: https://docs.rs/fastrand
