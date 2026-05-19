//! [`Deferred<A, E>`] — a one-shot, awaitable promise of an [`Exit<A, E>`].
//!
//! Anyone holding the deferred can call `succeed`, `fail`, or `complete`
//! once; subsequent completions are no-ops. Any number of `await_`
//! callers can suspend on it, and all receive the same completion when
//! it arrives.

use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::{Cause, Effect, Exit};

/// A one-shot promise, shareable across tasks.
pub struct Deferred<A, E> {
    state: Arc<Mutex<Option<Exit<A, E>>>>,
    notify: Arc<Notify>,
}

impl<A, E> Clone for Deferred<A, E> {
    fn clone(&self) -> Self {
        Deferred {
            state: self.state.clone(),
            notify: self.notify.clone(),
        }
    }
}

impl<A, E> Default for Deferred<A, E>
where
    A: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<A, E> Deferred<A, E>
where
    A: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
{
    /// Create a new, uncompleted deferred.
    pub fn new() -> Self {
        Deferred {
            state: Arc::new(Mutex::new(None)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Complete with a success value. Returns `true` if this call
    /// actually completed the deferred (`false` if already completed).
    pub fn succeed<E2, R>(&self, value: A) -> Effect<bool, E2, R>
    where
        E2: Send + 'static,
        R: Send + Sync + 'static,
    {
        self.complete(Exit::Success(value))
    }

    /// Complete with a typed failure. Returns `true` if this call set it.
    pub fn fail<E2, R>(&self, error: E) -> Effect<bool, E2, R>
    where
        E2: Send + 'static,
        R: Send + Sync + 'static,
    {
        self.complete(Exit::Failure(Cause::Fail(error)))
    }

    /// Complete with a pre-built [`Exit`] (for defects or interruption).
    pub fn complete<E2, R>(&self, exit: Exit<A, E>) -> Effect<bool, E2, R>
    where
        E2: Send + 'static,
        R: Send + Sync + 'static,
    {
        let state = self.state.clone();
        let notify = self.notify.clone();
        Effect::sync(move || {
            let mut guard = state.lock().expect("Deferred mutex poisoned");
            if guard.is_some() {
                Ok(false)
            } else {
                *guard = Some(exit.clone());
                notify.notify_waiters();
                Ok(true)
            }
        })
    }

    /// Suspend until the deferred completes, then replay the completion
    /// as this effect's result.
    pub fn await_<R>(&self) -> Effect<A, E, R>
    where
        R: Send + Sync + 'static,
    {
        let state = self.state.clone();
        let notify = self.notify.clone();
        Effect::from_fn_exit(move |_| {
            let state = state.clone();
            let notify = notify.clone();
            Box::pin(async move {
                loop {
                    // Register interest BEFORE checking state to avoid
                    // missing a completion that lands between the check
                    // and the await.
                    let notified = notify.notified();
                    tokio::pin!(notified);
                    {
                        let guard = state.lock().expect("Deferred mutex poisoned");
                        if let Some(exit) = guard.as_ref() {
                            return exit.clone();
                        }
                    }
                    notified.await;
                }
            })
        })
    }

    /// Non-blocking snapshot: `Some` if completed, `None` otherwise.
    pub fn poll<E2, R>(&self) -> Effect<Option<Exit<A, E>>, E2, R>
    where
        E2: Send + 'static,
        R: Send + Sync + 'static,
    {
        let state = self.state.clone();
        Effect::sync(move || Ok(state.lock().expect("Deferred mutex poisoned").clone()))
    }

    /// Whether this deferred has been completed.
    pub fn is_done<E2, R>(&self) -> Effect<bool, E2, R>
    where
        E2: Send + 'static,
        R: Send + Sync + 'static,
    {
        let state = self.state.clone();
        Effect::sync(move || Ok(state.lock().expect("Deferred mutex poisoned").is_some()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn newly_created_is_not_done() {
        let d: Deferred<i32, String> = Deferred::new();
        assert_eq!(d.is_done::<String, ()>().execute().await.ok(), Some(false));
        assert_eq!(d.poll::<String, ()>().execute().await.ok(), Some(None));
    }

    #[tokio::test]
    async fn succeed_then_await_returns_value() {
        let d: Deferred<i32, String> = Deferred::new();
        let set = d.succeed::<String, ()>(42).execute().await;
        assert_eq!(set.ok(), Some(true));
        let got = d.await_::<()>().execute().await;
        assert_eq!(got.ok(), Some(42));
    }

    #[tokio::test]
    async fn fail_replays_as_typed_failure() {
        let d: Deferred<i32, String> = Deferred::new();
        d.fail::<String, ()>("boom".into()).execute().await;
        let got = d.await_::<()>().execute().await;
        assert_eq!(got.err(), Some("boom".to_string()));
    }

    #[tokio::test]
    async fn complete_with_die_replays_as_die() {
        let d: Deferred<i32, String> = Deferred::new();
        d.complete::<String, ()>(Exit::Failure(Cause::Die(
            crate::Defect::new("ouch"),
        )))
        .execute()
        .await;
        match d.await_::<()>().execute().await {
            Exit::Failure(Cause::Die(def)) => assert_eq!(def.message, "ouch"),
            other => panic!("expected Die, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn second_complete_is_a_noop() {
        let d: Deferred<i32, String> = Deferred::new();
        let first = d.succeed::<String, ()>(1).execute().await;
        let second = d.succeed::<String, ()>(2).execute().await;
        assert_eq!(first.ok(), Some(true));
        assert_eq!(second.ok(), Some(false));
        assert_eq!(d.await_::<()>().execute().await.ok(), Some(1));
    }

    #[tokio::test]
    async fn await_suspends_until_completed() {
        let d: Deferred<i32, String> = Deferred::new();
        let d_writer = d.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            d_writer.succeed::<String, ()>(7).execute().await;
        });
        let got = d.await_::<()>().execute().await;
        assert_eq!(got.ok(), Some(7));
    }

    #[tokio::test]
    async fn multiple_awaiters_all_receive_the_value() {
        let d: Deferred<i32, String> = Deferred::new();
        let mut handles = Vec::new();
        for _ in 0..5 {
            let dd = d.clone();
            handles.push(tokio::spawn(async move {
                dd.await_::<()>().execute().await
            }));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        d.succeed::<String, ()>(99).execute().await;
        for h in handles {
            let exit = h.await.unwrap();
            assert_eq!(exit.ok(), Some(99));
        }
    }
}
