//! [`Ref<A>`] — a cheap, cloneable handle to shared mutable state with
//! effect-typed read/update operations.
//!
//! `Ref` operations are infallible (they never produce `Cause::Fail`) but
//! their `E` and `R` parameters are generic so they fit into any
//! surrounding `Effect<_, E, R>` chain without conversion.

use std::sync::{Arc, Mutex};

use crate::Effect;

/// A shared, mutable cell.
///
/// ```ignore
/// let counter = Ref::new(0);
/// let program = counter
///     .update::<String, ()>(|n| n + 1)
///     .flat_map(|_| counter.get());
/// assert_eq!(program.execute().await.ok(), Some(1));
/// ```
pub struct Ref<A> {
    inner: Arc<Mutex<A>>,
}

impl<A> Clone for Ref<A> {
    fn clone(&self) -> Self {
        Ref { inner: self.inner.clone() }
    }
}

impl<A> Ref<A>
where
    A: Send + 'static,
{
    /// Construct a new ref with the given initial value.
    pub fn new(initial: A) -> Self {
        Ref { inner: Arc::new(Mutex::new(initial)) }
    }

    /// Read the current value.
    pub fn get<E, R>(&self) -> Effect<A, E, R>
    where
        A: Clone + Sync,
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        let inner = self.inner.clone();
        Effect::sync(move || Ok(inner.lock().expect("Ref mutex poisoned").clone()))
    }

    /// Overwrite the current value.
    pub fn set<E, R>(&self, value: A) -> Effect<(), E, R>
    where
        A: Clone + Sync,
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        let inner = self.inner.clone();
        Effect::sync(move || {
            *inner.lock().expect("Ref mutex poisoned") = value.clone();
            Ok(())
        })
    }

    /// Apply `f` to the current value and store the result.
    pub fn update<E, R, F>(&self, f: F) -> Effect<(), E, R>
    where
        A: Clone + Sync,
        E: Send + 'static,
        R: Send + Sync + 'static,
        F: Fn(A) -> A + Send + Sync + 'static,
    {
        let inner = self.inner.clone();
        Effect::sync(move || {
            let mut guard = inner.lock().expect("Ref mutex poisoned");
            let new = f(guard.clone());
            *guard = new;
            Ok(())
        })
    }

    /// Atomically apply `f`, storing the new value and returning the
    /// extracted output. The classic "compute B from A and a transition
    /// of A" pattern.
    pub fn modify<E, R, B, F>(&self, f: F) -> Effect<B, E, R>
    where
        A: Clone + Sync,
        B: Send + 'static,
        E: Send + 'static,
        R: Send + Sync + 'static,
        F: Fn(A) -> (B, A) + Send + Sync + 'static,
    {
        let inner = self.inner.clone();
        Effect::sync(move || {
            let mut guard = inner.lock().expect("Ref mutex poisoned");
            let (output, new_value) = f(guard.clone());
            *guard = new_value;
            Ok(output)
        })
    }

    /// Atomically replace the value, returning the old one.
    pub fn get_and_set<E, R>(&self, value: A) -> Effect<A, E, R>
    where
        A: Clone + Sync,
        E: Send + 'static,
        R: Send + Sync + 'static,
    {
        let inner = self.inner.clone();
        Effect::sync(move || {
            let mut guard = inner.lock().expect("Ref mutex poisoned");
            Ok(std::mem::replace(&mut *guard, value.clone()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_returns_initial_value() {
        let r = Ref::new(42_i32);
        assert_eq!(r.get::<String, ()>().execute().await.ok(), Some(42));
    }

    #[tokio::test]
    async fn set_overwrites_value() {
        let r = Ref::new(0_i32);
        r.set::<String, ()>(7).execute().await;
        assert_eq!(r.get::<String, ()>().execute().await.ok(), Some(7));
    }

    #[tokio::test]
    async fn update_applies_function() {
        let r = Ref::new(10_i32);
        r.update::<String, (), _>(|n| n + 5).execute().await;
        assert_eq!(r.get::<String, ()>().execute().await.ok(), Some(15));
    }

    #[tokio::test]
    async fn modify_extracts_output_and_stores_new() {
        let r = Ref::new(10_i32);
        let exit = r
            .modify::<String, (), _, _>(|n| (format!("was {n}"), n + 1))
            .execute()
            .await;
        assert_eq!(exit.ok(), Some("was 10".to_string()));
        assert_eq!(r.get::<String, ()>().execute().await.ok(), Some(11));
    }

    #[tokio::test]
    async fn get_and_set_returns_previous_value() {
        let r = Ref::new(1_i32);
        let exit = r.get_and_set::<String, ()>(99).execute().await;
        assert_eq!(exit.ok(), Some(1));
        assert_eq!(r.get::<String, ()>().execute().await.ok(), Some(99));
    }

    #[tokio::test]
    async fn ref_is_cloneable_and_shares_state() {
        let a = Ref::new(0_i32);
        let b = a.clone();
        a.set::<String, ()>(7).execute().await;
        assert_eq!(b.get::<String, ()>().execute().await.ok(), Some(7));
    }

    #[tokio::test]
    async fn concurrent_updates_sum_correctly() {
        let counter = Ref::new(0_i32);
        let mut handles = Vec::new();
        for _ in 0..100 {
            let c = counter.clone();
            handles.push(tokio::spawn(async move {
                c.update::<String, (), _>(|n| n + 1).execute().await;
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert_eq!(counter.get::<String, ()>().execute().await.ok(), Some(100));
    }
}
