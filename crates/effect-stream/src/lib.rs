//! Effect-typed async streams.
//!
//! A [`Stream<A, E, R>`] is a *description* of a producer of `A`
//! values that may fail with `E` and requires environment `R`.
//! Building a stream is pure; running it (via `run_collect`,
//! `run_for_each`, etc.) produces an [`Effect`] that drives it.
//!
//! Under the hood this wraps [`futures::Stream`]; the bridge lets us
//! reuse the futures ecosystem (combinators, IO adapters) while
//! presenting a consistent Effect-typed surface.

use std::sync::Arc;

use effect::Effect;
use futures::StreamExt;
use futures::stream::BoxStream;

/// A lazy, effect-typed async producer.
pub struct Stream<A, E, R> {
    factory:
        Arc<dyn Fn(Arc<R>) -> BoxStream<'static, Result<A, E>> + Send + Sync>,
}

impl<A, E, R> Clone for Stream<A, E, R> {
    fn clone(&self) -> Self {
        Stream { factory: self.factory.clone() }
    }
}

// ── Constructors ─────────────────────────────────────────────────

impl<A, E, R> Stream<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// A stream that emits nothing and completes immediately.
    pub fn empty() -> Self {
        Stream {
            factory: Arc::new(|_| futures::stream::empty().boxed()),
        }
    }

    /// A stream that emits exactly one value and completes.
    pub fn single(value: A) -> Self
    where
        A: Clone + Sync,
    {
        Stream {
            factory: Arc::new(move |_| {
                let v = value.clone();
                futures::stream::once(async move { Ok::<A, E>(v) }).boxed()
            }),
        }
    }

    /// A stream that immediately fails (emits nothing).
    pub fn fail(error: E) -> Self
    where
        E: Clone + Sync,
    {
        Stream {
            factory: Arc::new(move |_| {
                let e = error.clone();
                futures::stream::once(async move { Err::<A, E>(e) }).boxed()
            }),
        }
    }

    /// A stream over a cloneable iterable.
    pub fn from_iter<I>(items: I) -> Self
    where
        I: IntoIterator<Item = A> + Clone + Send + Sync + 'static,
        I::IntoIter: Send + 'static,
    {
        Stream {
            factory: Arc::new(move |_| {
                let items = items.clone();
                futures::stream::iter(items.into_iter().map(Ok)).boxed()
            }),
        }
    }
}

// ── Transformations ──────────────────────────────────────────────

impl<A, E, R> Stream<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// Transform each emitted item via a pure function.
    pub fn map<B, F>(self, f: F) -> Stream<B, E, R>
    where
        B: Send + 'static,
        F: Fn(A) -> B + Send + Sync + 'static,
    {
        let factory = self.factory;
        let f = Arc::new(f);
        Stream {
            factory: Arc::new(move |r| {
                let f = f.clone();
                factory(r)
                    .map(move |item| item.map(|a| (f)(a)))
                    .boxed()
            }),
        }
    }

    /// Transform each emitted item via an effect. The effect runs with
    /// the same environment as the stream. Defects and interruption
    /// inside the per-item effect propagate as **panics** — use
    /// `map_effect_exit` (TODO) for full cause handling.
    pub fn map_effect<B, F>(self, f: F) -> Stream<B, E, R>
    where
        B: Send + 'static,
        E: std::fmt::Debug,
        F: Fn(A) -> Effect<B, E, R> + Send + Sync + 'static,
    {
        let factory = self.factory;
        let f = Arc::new(f);
        Stream {
            factory: Arc::new(move |r| {
                let r_outer = r.clone();
                let f = f.clone();
                factory(r)
                    .then(move |item| {
                        let r = r_outer.clone();
                        let f = f.clone();
                        async move {
                            match item {
                                Ok(a) => (f)(a).run(r).await.into_typed_result(),
                                Err(e) => Err(e),
                            }
                        }
                    })
                    .boxed()
            }),
        }
    }

    /// Keep only items satisfying `pred`. Errors pass through unchanged.
    pub fn filter<F>(self, pred: F) -> Self
    where
        A: Sync,
        F: Fn(&A) -> bool + Send + Sync + 'static,
    {
        let factory = self.factory;
        let pred = Arc::new(pred);
        Stream {
            factory: Arc::new(move |r| {
                let pred = pred.clone();
                factory(r)
                    .filter(move |item| {
                        let keep = match item {
                            Ok(a) => pred(a),
                            Err(_) => true,
                        };
                        async move { keep }
                    })
                    .boxed()
            }),
        }
    }

    /// Take at most `n` items.
    pub fn take(self, n: usize) -> Self {
        let factory = self.factory;
        Stream {
            factory: Arc::new(move |r| factory(r).take(n).boxed()),
        }
    }

    /// Skip the first `n` items.
    pub fn drop(self, n: usize) -> Self {
        let factory = self.factory;
        Stream {
            factory: Arc::new(move |r| factory(r).skip(n).boxed()),
        }
    }

    /// Concatenate two streams (self first, then `other`).
    pub fn concat(self, other: Self) -> Self {
        let f1 = self.factory;
        let f2 = other.factory;
        Stream {
            factory: Arc::new(move |r| {
                let r2 = r.clone();
                f1(r).chain(f2(r2)).boxed()
            }),
        }
    }
}

// ── Terminals (produce Effect) ───────────────────────────────────

impl<A, E, R> Stream<A, E, R>
where
    A: Send + 'static,
    E: Send + 'static,
    R: Send + Sync + 'static,
{
    /// Run to completion, collecting every emitted item into a `Vec`.
    /// Fails fast on the first error.
    pub fn run_collect(self) -> Effect<Vec<A>, E, R> {
        let factory = self.factory;
        Effect::from_fn(move |r| {
            let mut stream = factory(r);
            async move {
                let mut out = Vec::new();
                while let Some(item) = stream.next().await {
                    out.push(item?);
                }
                Ok(out)
            }
        })
    }

    /// Run to completion, applying `f` to every successful item.
    pub fn run_for_each<F>(self, f: F) -> Effect<(), E, R>
    where
        F: Fn(A) + Send + Sync + 'static,
    {
        let factory = self.factory;
        let f = Arc::new(f);
        Effect::from_fn(move |r| {
            let f = f.clone();
            let mut stream = factory(r);
            async move {
                while let Some(item) = stream.next().await {
                    let a = item?;
                    f(a);
                }
                Ok(())
            }
        })
    }

    /// Run to completion, discarding every emitted item.
    pub fn run_drain(self) -> Effect<(), E, R> {
        self.run_for_each(|_| {})
    }

    /// Left-fold the stream.
    pub fn run_fold<B, F>(self, init: B, f: F) -> Effect<B, E, R>
    where
        B: Clone + Send + Sync + 'static,
        F: Fn(B, A) -> B + Send + Sync + 'static,
    {
        let factory = self.factory;
        let f = Arc::new(f);
        Effect::from_fn(move |r| {
            let f = f.clone();
            let init = init.clone();
            let mut stream = factory(r);
            async move {
                let mut acc = init;
                while let Some(item) = stream.next().await {
                    let a = item?;
                    acc = (*f)(acc, a);
                }
                Ok(acc)
            }
        })
    }

    /// Take the first item, if any. Subsequent items are not produced.
    pub fn run_head(self) -> Effect<Option<A>, E, R> {
        let factory = self.factory;
        Effect::from_fn(move |r| {
            let mut stream = factory(r);
            async move {
                match stream.next().await {
                    Some(Ok(a)) => Ok(Some(a)),
                    Some(Err(e)) => Err(e),
                    None => Ok(None),
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use effect::Effect;

    #[tokio::test]
    async fn empty_collects_to_empty_vec() {
        let s: Stream<i32, String, ()> = Stream::empty();
        let result = s.run_collect().execute().await;
        assert_eq!(result.ok(), Some(vec![]));
    }

    #[tokio::test]
    async fn single_yields_one_item() {
        let s: Stream<i32, String, ()> = Stream::single(42);
        let result = s.run_collect().execute().await;
        assert_eq!(result.ok(), Some(vec![42]));
    }

    #[tokio::test]
    async fn from_iter_yields_all_in_order() {
        let s: Stream<i32, String, ()> = Stream::from_iter(vec![1, 2, 3, 4, 5]);
        let result = s.run_collect().execute().await;
        assert_eq!(result.ok(), Some(vec![1, 2, 3, 4, 5]));
    }

    #[tokio::test]
    async fn fail_emits_no_items_and_errors() {
        let s: Stream<i32, String, ()> = Stream::fail("boom".into());
        let result = s.run_collect().execute().await;
        assert_eq!(result.err(), Some("boom".to_string()));
    }

    #[tokio::test]
    async fn map_transforms_items() {
        let s: Stream<i32, String, ()> = Stream::from_iter(vec![1, 2, 3]).map(|x| x * 10);
        let result = s.run_collect().execute().await;
        assert_eq!(result.ok(), Some(vec![10, 20, 30]));
    }

    #[tokio::test]
    async fn map_effect_runs_per_item_effect() {
        let s: Stream<i32, String, ()> = Stream::from_iter(vec![1, 2, 3])
            .map_effect(|x| Effect::succeed(x + 100));
        let result = s.run_collect().execute().await;
        assert_eq!(result.ok(), Some(vec![101, 102, 103]));
    }

    #[tokio::test]
    async fn map_effect_propagates_typed_failure() {
        let s: Stream<i32, String, ()> = Stream::from_iter(vec![1, 2, 3])
            .map_effect(|x| {
                if x == 2 {
                    Effect::<i32, String, ()>::fail("nope".into())
                } else {
                    Effect::succeed(x)
                }
            });
        let result = s.run_collect().execute().await;
        assert_eq!(result.err(), Some("nope".to_string()));
    }

    #[tokio::test]
    async fn filter_keeps_matching() {
        let s: Stream<i32, String, ()> =
            Stream::from_iter(vec![1, 2, 3, 4, 5]).filter(|x| x % 2 == 0);
        let result = s.run_collect().execute().await;
        assert_eq!(result.ok(), Some(vec![2, 4]));
    }

    #[tokio::test]
    async fn take_limits_count() {
        let s: Stream<i32, String, ()> = Stream::from_iter(vec![1, 2, 3, 4, 5]).take(3);
        let result = s.run_collect().execute().await;
        assert_eq!(result.ok(), Some(vec![1, 2, 3]));
    }

    #[tokio::test]
    async fn drop_skips_prefix() {
        let s: Stream<i32, String, ()> = Stream::from_iter(vec![1, 2, 3, 4, 5]).drop(2);
        let result = s.run_collect().execute().await;
        assert_eq!(result.ok(), Some(vec![3, 4, 5]));
    }

    #[tokio::test]
    async fn concat_appends_second_to_first() {
        let a: Stream<i32, String, ()> = Stream::from_iter(vec![1, 2]);
        let b: Stream<i32, String, ()> = Stream::from_iter(vec![3, 4]);
        let result = a.concat(b).run_collect().execute().await;
        assert_eq!(result.ok(), Some(vec![1, 2, 3, 4]));
    }

    #[tokio::test]
    async fn run_fold_reduces() {
        let s: Stream<i32, String, ()> = Stream::from_iter(vec![1, 2, 3, 4]);
        let result = s.run_fold(0, |acc, x| acc + x).execute().await;
        assert_eq!(result.ok(), Some(10));
    }

    #[tokio::test]
    async fn run_for_each_visits_every_item() {
        use std::sync::Mutex;
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();
        let s: Stream<i32, String, ()> = Stream::from_iter(vec![1, 2, 3]);
        s.run_for_each(move |x| seen_clone.lock().unwrap().push(x))
            .execute()
            .await;
        assert_eq!(*seen.lock().unwrap(), vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn run_head_returns_first() {
        let s: Stream<i32, String, ()> = Stream::from_iter(vec![10, 20, 30]);
        let result = s.run_head().execute().await;
        assert_eq!(result.ok(), Some(Some(10)));
    }

    #[tokio::test]
    async fn run_head_on_empty_returns_none() {
        let s: Stream<i32, String, ()> = Stream::empty();
        let result = s.run_head().execute().await;
        assert_eq!(result.ok(), Some(None));
    }

    #[tokio::test]
    async fn run_drain_completes_silently() {
        let s: Stream<i32, String, ()> = Stream::from_iter(vec![1, 2, 3]);
        let result = s.run_drain().execute().await;
        assert_eq!(result.ok(), Some(()));
    }

    #[tokio::test]
    async fn pipeline_composes() {
        let s: Stream<i32, String, ()> = Stream::from_iter(1..=10)
            .map(|x| x * 2)
            .filter(|x| *x > 5)
            .take(3);
        let result = s.run_collect().execute().await;
        assert_eq!(result.ok(), Some(vec![6, 8, 10]));
    }
}
