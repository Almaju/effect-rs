//! `Random` — a service for non-cryptographic randomness. Live impl
//! uses [`fastrand`], which is thread-local-seeded and fine for
//! shuffling/sampling/test data. Cryptographically-secure random is
//! a future addition.

use std::sync::Arc;

use effect::Effect;

/// A random-number service. Bind `R` by this to pull random values
/// inside an effect.
pub trait Random: Send + Sync + 'static {
    fn next_u64(&self) -> u64;
    fn next_f64(&self) -> f64;
    fn fill_bytes(&self, buf: &mut [u8]);
}

/// Live `fastrand`-backed implementation.
pub struct LiveRandom;

impl Random for LiveRandom {
    fn next_u64(&self) -> u64 {
        fastrand::u64(..)
    }
    fn next_f64(&self) -> f64 {
        fastrand::f64()
    }
    fn fill_bytes(&self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            *b = fastrand::u8(..);
        }
    }
}

// ── Effects ──────────────────────────────────────────────────────

pub fn next_u64<E, R>() -> Effect<u64, E, R>
where
    E: Send + 'static,
    R: Random,
{
    Effect::from_fn(|r: Arc<R>| async move { Ok(r.next_u64()) })
}

pub fn next_f64<E, R>() -> Effect<f64, E, R>
where
    E: Send + 'static,
    R: Random,
{
    Effect::from_fn(|r: Arc<R>| async move { Ok(r.next_f64()) })
}

/// Random unsigned integer in `[0, exclusive_max)`. Returns 0 when
/// `exclusive_max == 0`.
pub fn range_u64<E, R>(exclusive_max: u64) -> Effect<u64, E, R>
where
    E: Send + 'static,
    R: Random,
{
    Effect::from_fn(move |r: Arc<R>| async move {
        if exclusive_max == 0 {
            return Ok(0);
        }
        Ok(r.next_u64() % exclusive_max)
    })
}

pub fn fill_bytes<E, R>(len: usize) -> Effect<Vec<u8>, E, R>
where
    E: Send + 'static,
    R: Random,
{
    Effect::from_fn(move |r: Arc<R>| async move {
        let mut buf = vec![0u8; len];
        r.fill_bytes(&mut buf);
        Ok(buf)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Deterministic counter "RNG" for tests.
    pub struct CountingRandom {
        counter: Mutex<u64>,
    }
    impl CountingRandom {
        pub fn new() -> Self {
            CountingRandom { counter: Mutex::new(0) }
        }
    }
    impl Random for CountingRandom {
        fn next_u64(&self) -> u64 {
            let mut c = self.counter.lock().unwrap();
            *c += 1;
            *c
        }
        fn next_f64(&self) -> f64 {
            self.next_u64() as f64
        }
        fn fill_bytes(&self, buf: &mut [u8]) {
            for b in buf.iter_mut() {
                *b = (self.next_u64() & 0xFF) as u8;
            }
        }
    }

    #[tokio::test]
    async fn next_u64_uses_context() {
        let r = CountingRandom::new();
        let exit = next_u64::<String, CountingRandom>().run_with(r).await;
        assert_eq!(exit.ok(), Some(1));
    }

    #[tokio::test]
    async fn live_random_produces_values_in_range() {
        let exit = range_u64::<String, LiveRandom>(100)
            .run_with(LiveRandom)
            .await;
        let v = exit.ok().unwrap();
        assert!(v < 100);
    }

    #[tokio::test]
    async fn fill_bytes_writes_requested_length() {
        let exit = fill_bytes::<String, LiveRandom>(16)
            .run_with(LiveRandom)
            .await;
        let bytes = exit.ok().unwrap();
        assert_eq!(bytes.len(), 16);
    }

    #[tokio::test]
    async fn range_u64_with_zero_returns_zero() {
        let r = CountingRandom::new();
        let exit = range_u64::<String, CountingRandom>(0).run_with(r).await;
        assert_eq!(exit.ok(), Some(0));
    }
}
