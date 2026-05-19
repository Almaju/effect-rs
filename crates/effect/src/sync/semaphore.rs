//! [`Semaphore`] — a counting semaphore for bounding concurrency.
//!
//! Wraps `tokio::sync::Semaphore`. The most common entry point is
//! [`Semaphore::with_permit`] which acquires one permit, runs the inner
//! effect, and releases the permit automatically — even if the inner
//! effect fails.

use std::sync::Arc;

use crate::{Cause, Defect, Effect, Exit};

pub struct Semaphore {
    inner: Arc<tokio::sync::Semaphore>,
}

impl Clone for Semaphore {
    fn clone(&self) -> Self {
        Semaphore { inner: self.inner.clone() }
    }
}

impl Semaphore {
    /// Construct a semaphore with the given initial permit count.
    pub fn new(permits: usize) -> Self {
        Semaphore { inner: Arc::new(tokio::sync::Semaphore::new(permits)) }
    }

    /// Number of permits currently available.
    pub fn available_permits<E, R>(&self) -> Effect<usize, E, R>
    where
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        let inner = self.inner.clone();
        Effect::sync(move || Ok(inner.available_permits()))
    }

    /// Try to acquire one permit without suspending. Returns `true` if
    /// acquired. The permit is **released immediately** on the way out —
    /// this method is only useful as a probe.
    pub fn try_acquire<E, R>(&self) -> Effect<bool, E, R>
    where
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        let inner = self.inner.clone();
        Effect::sync(move || Ok(inner.try_acquire().is_ok()))
    }

    /// Run `eff` while holding one permit. Suspends until a permit is
    /// available; releases automatically when `eff` finishes (success
    /// or failure).
    pub fn with_permit<A, E, R>(&self, eff: Effect<A, E, R>) -> Effect<A, E, R>
    where
        A: Send + 'static,
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        self.with_permits(1, eff)
    }

    /// Run `eff` while holding `n` permits at once.
    pub fn with_permits<A, E, R>(&self, n: u32, eff: Effect<A, E, R>) -> Effect<A, E, R>
    where
        A: Send + 'static,
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        let inner = self.inner.clone();
        let eff_run = eff.run_fn;
        Effect {
            run_fn: Arc::new(move |r| {
                let inner = inner.clone();
                let eff_run = eff_run.clone();
                Box::pin(async move {
                    match inner.acquire_many_owned(n).await {
                        Ok(_permit) => {
                            let out = eff_run(r).await;
                            // _permit dropped here, releasing the semaphore.
                            out
                        }
                        Err(_) => Exit::Failure(Cause::Die(Defect::new("semaphore closed"))),
                    }
                })
            }),
        }
    }

    /// Close the semaphore. Subsequent acquire attempts fail with a
    /// `Cause::Die`.
    pub fn close<E, R>(&self) -> Effect<(), E, R>
    where
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        let inner = self.inner.clone();
        Effect::sync(move || {
            inner.close();
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn try_acquire_succeeds_when_permits_available() {
        let sem = Semaphore::new(2);
        assert_eq!(sem.try_acquire::<String, ()>().execute().await.ok(), Some(true));
    }

    #[tokio::test]
    async fn with_permit_runs_effect() {
        let sem = Semaphore::new(1);
        let eff = Effect::<i32, String, ()>::succeed(42);
        let exit = sem.with_permit(eff).execute().await;
        assert_eq!(exit.ok(), Some(42));
    }

    #[tokio::test]
    async fn with_permit_releases_after_success() {
        let sem = Semaphore::new(1);
        let _ = sem.with_permit(Effect::<i32, String, ()>::succeed(1)).execute().await;
        // permit released — try_acquire should still succeed.
        assert_eq!(sem.try_acquire::<String, ()>().execute().await.ok(), Some(true));
    }

    #[tokio::test]
    async fn with_permit_releases_after_failure() {
        let sem = Semaphore::new(1);
        let _ = sem
            .with_permit(Effect::<i32, String, ()>::fail("boom".to_string()))
            .execute()
            .await;
        assert_eq!(sem.try_acquire::<String, ()>().execute().await.ok(), Some(true));
    }

    #[tokio::test]
    async fn limits_concurrent_effects() {
        let sem = Semaphore::new(2);
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..10 {
            let sem = sem.clone();
            let in_flight = in_flight.clone();
            let max_in_flight = max_in_flight.clone();
            handles.push(tokio::spawn(async move {
                let work = Effect::<(), String, ()>::from_fn(move |_| {
                    let in_flight = in_flight.clone();
                    let max_in_flight = max_in_flight.clone();
                    async move {
                        let current = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                        max_in_flight.fetch_max(current, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                        Ok(())
                    }
                });
                sem.with_permit(work).execute().await;
            }));
        }
        for h in handles { h.await.unwrap(); }
        assert!(max_in_flight.load(Ordering::SeqCst) <= 2);
    }

    #[tokio::test]
    async fn close_causes_acquire_to_die() {
        let sem = Semaphore::new(1);
        sem.close::<String, ()>().execute().await;
        let exit = sem
            .with_permit(Effect::<i32, String, ()>::succeed(0))
            .execute()
            .await;
        assert!(matches!(exit, Exit::Failure(Cause::Die(_))));
    }
}
