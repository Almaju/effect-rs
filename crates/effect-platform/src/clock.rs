//! `Clock` — a service for reading wall-clock time and a monotonic
//! instant, plus a non-Clock-requiring `sleep` helper that drives
//! tokio's timer directly.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use effect::Effect;

/// A clock service. Bind `R` by this to read time inside an effect.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> SystemTime;
    fn monotonic(&self) -> Instant;
}

/// Marker re-export for symmetry — `ClockServices` is currently just
/// `Clock`. Bound on `R: ClockServices` reads as "any clock will do".
pub trait ClockServices: Clock {}
impl<T: Clock> ClockServices for T {}

/// The real clock backed by `std::time::SystemTime` / `Instant`.
pub struct LiveClock;

impl Clock for LiveClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
    fn monotonic(&self) -> Instant {
        Instant::now()
    }
}

// ── Effects ──────────────────────────────────────────────────────

/// Read the current wall-clock time from the context's [`Clock`].
pub fn now<E, R>() -> Effect<SystemTime, E, R>
where
    E: Send + 'static,
    R: Clock,
{
    Effect::from_fn(|r: Arc<R>| async move { Ok(r.now()) })
}

/// Read a monotonic `Instant` from the context's [`Clock`].
pub fn monotonic<E, R>() -> Effect<Instant, E, R>
where
    E: Send + 'static,
    R: Clock,
{
    Effect::from_fn(|r: Arc<R>| async move { Ok(r.monotonic()) })
}

/// Suspend for at least `duration`. Doesn't require a [`Clock`] —
/// uses tokio's timer directly.
pub fn sleep<E, R>(duration: Duration) -> Effect<(), E, R>
where
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    Effect::from_fn(move |_| async move {
        tokio::time::sleep(duration).await;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Fake clock that returns canned values — for tests.
    pub struct FakeClock {
        pub instants: Mutex<Vec<Instant>>,
    }

    impl FakeClock {
        pub fn new() -> Self {
            FakeClock {
                instants: Mutex::new(vec![Instant::now()]),
            }
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> SystemTime {
            SystemTime::UNIX_EPOCH + Duration::from_secs(1000)
        }
        fn monotonic(&self) -> Instant {
            let mut g = self.instants.lock().unwrap();
            let last = *g.last().unwrap();
            let next = last + Duration::from_millis(10);
            g.push(next);
            last
        }
    }

    #[tokio::test]
    async fn now_reads_from_live_clock() {
        let before = SystemTime::now();
        let exit = now::<String, LiveClock>()
            .run_with(LiveClock)
            .await;
        let read = exit.ok().unwrap();
        let after = SystemTime::now();
        assert!(read >= before && read <= after);
    }

    #[tokio::test]
    async fn now_reads_from_fake_clock() {
        let fake = FakeClock::new();
        let exit = now::<String, FakeClock>().run_with(fake).await;
        let read = exit.ok().unwrap();
        let expected = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        assert_eq!(read, expected);
    }

    #[tokio::test]
    async fn monotonic_advances_per_read() {
        let fake = FakeClock::new();
        let a = monotonic::<String, FakeClock>()
            .run(Arc::new(fake))
            .await
            .ok()
            .unwrap();
        // Subsequent reads on the same Arc would advance — single-read
        // smoke test here.
        assert!(a <= Instant::now());
    }

    #[tokio::test]
    async fn sleep_actually_waits() {
        let start = Instant::now();
        let _ = sleep::<String, ()>(Duration::from_millis(30))
            .execute()
            .await;
        assert!(start.elapsed() >= Duration::from_millis(25));
    }
}
